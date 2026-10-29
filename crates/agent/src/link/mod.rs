//! link 的 agent 侧：启动 executor 子进程，按请求 id 多路复用帧。
//! `ToolBox` 实现在 `tools`；帧与工具体现在 `crates/core` 与 `crates/executor`。
//!
//! 规格 user story R-35：同一 host 上的多个 session 共用一条 link，请求按 id 配对。
//! 断链时的退出码与标准错误尾部记在 [`LinkClosed`]；host 的 `ok`/`unreachable`/`incompatible`
//! 三态由 `crate::hosts` 按这些数据装配，本模块只分类、不宣布状态。
//!
//! link 上有两种请求：工具执行与 workspace 校验。两者都是一帧请求、一帧结果，
//! 答复种类与请求操作一一对应。

use std::collections::{HashMap, VecDeque};
use std::io;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use durable::link::{
    Frame, LINK_PROTOCOL_VERSION, Operation, Reply, WorkspaceCheckError, read_frame, write_frame,
};
use durable::{Cancellation, ToolErrorCode, ToolLimits, ToolOutcome};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::sync::{Mutex, mpsc, oneshot, watch};

mod closed;
mod tools;

pub use closed::{INCOMPATIBLE_EXIT_CODE, LinkClosed};

pub use tools::{NoTools, RemoteTools, ToolBox, ToolCall, ToolContext};

/// 待写帧队列容量。写侧只有一个任务，容量只用于吸收突发。
const FRAME_QUEUE: usize = 64;

/// 断链说明里保留的标准错误行数上限。
const STDERR_TAIL_LINES: usize = 20;

/// 读侧等到子进程退出码的时限。超时仍按断链处理，只是没有退出码。
const EXIT_WAIT: Duration = Duration::from_secs(2);

/// 子进程退出后等待标准错误读尽的时限。
const STDERR_DRAIN: Duration = Duration::from_secs(1);

/// 在途请求的等待者。答复种类与请求操作一一对应，由调用方解释。
type Waiter = oneshot::Sender<Result<Reply, LinkClosed>>;

/// 在途请求表，按请求 id 索引。
type Pending = HashMap<String, Waiter>;

/// 心跳等待者：发出时刻与应答通道。
type Pong = (Instant, oneshot::Sender<u64>);

/// 在途请求与断链原因。两者同一把锁，避免「刚登记就错过断链」的竞态。
#[derive(Default)]
struct Requests {
    closed: Option<LinkClosed>,
    pending: Pending,
}

/// 一条 link 的共享状态：读侧、写侧、退出观察者与在途请求共看这一份。
struct LinkState {
    requests: Mutex<Requests>,
    stderr_tail: Mutex<VecDeque<String>>,
    /// 断链广播。`wait_closed` 订阅它，因此不依赖轮询。
    closed: watch::Sender<Option<LinkClosed>>,
}

impl Default for LinkState {
    fn default() -> Self {
        let (closed, _receiver) = watch::channel(None);
        Self {
            requests: Mutex::default(),
            stderr_tail: Mutex::default(),
            closed,
        }
    }
}

impl LinkState {
    /// 当前断链原因。
    async fn closed(&self) -> Option<LinkClosed> {
        self.requests.lock().await.closed.clone()
    }

    /// 订阅断链广播。
    fn subscribe(&self) -> watch::Receiver<Option<LinkClosed>> {
        self.closed.subscribe()
    }

    /// 登记在途请求。link 已断开时不再登记，直接返回断链原因。
    async fn register(&self, id: String, sender: Waiter) -> Result<(), LinkClosed> {
        let mut requests = self.requests.lock().await;
        if let Some(closed) = requests.closed.as_ref() {
            return Err(closed.clone());
        }
        requests.pending.insert(id, sender);
        Ok(())
    }

    /// 不再等待该请求。
    async fn forget(&self, id: &str) {
        drop(self.requests.lock().await.pending.remove(id));
    }

    /// 读侧收到的一帧结果交给对应等待者。
    async fn resolve(&self, id: &str, reply: Reply) {
        let waiter = self.requests.lock().await.pending.remove(id);
        if let Some(sender) = waiter {
            if sender.send(Ok(reply)).is_err() {
                tracing::debug!(id = %id, "请求结果没有等待者");
            }
        } else {
            tracing::debug!(id = %id, "收到不匹配的请求结果");
        }
    }

    /// 是否有在途请求。心跳只在空闲 link 上发。
    async fn is_idle(&self) -> bool {
        self.requests.lock().await.pending.is_empty()
    }

    /// 记录断链，并取出全部在途等待者。先到者决定 [`LinkClosed`]，后来者只读取。
    async fn close(&self, exit_code: Option<i32>) -> (LinkClosed, Vec<Waiter>) {
        let stderr_tail = self
            .stderr_tail
            .lock()
            .await
            .iter()
            .cloned()
            .collect::<Vec<String>>()
            .join("\n");
        let mut requests = self.requests.lock().await;
        if requests.closed.is_none() {
            requests.closed = Some(LinkClosed {
                exit_code,
                stderr_tail,
            });
        }
        let closed = requests.closed.clone().unwrap_or_else(LinkClosed::lost);
        let waiters = requests.pending.drain().map(|(_, sender)| sender).collect();
        drop(requests);
        if self.closed.send(Some(closed.clone())).is_err() {
            tracing::debug!("断链广播没有订阅者");
        }
        (closed, waiters)
    }
}

/// 一条本地 link：executor 子进程、写侧任务、读侧任务、退出观察者与在途请求表。
///
/// 一个 host 只有一条 link，多个 session 共用它；请求按 id 多路复用。
pub struct LinkHandle {
    /// 丢弃本结构时要求退出观察者结束子进程。
    terminate: Option<oneshot::Sender<()>>,
    writer: mpsc::Sender<Frame>,
    state: Arc<LinkState>,
    pings: Arc<Mutex<HashMap<u64, Pong>>>,
    next_request: AtomicU64,
    pid: Option<u32>,
}

impl LinkHandle {
    /// 启动本机传输：直接拉起 executor 子进程，标准输入输出即 link。
    ///
    /// 工具上限随启动参数交给 executor，因此两侧看到的截断与超时一致。
    ///
    /// # Errors
    ///
    /// - executor 无法启动或缺少标准输入输出管道时返回该错误。
    pub fn spawn_local(executor_path: &Path, limits: ToolLimits) -> io::Result<Self> {
        let mut child = Command::new(executor_path)
            .arg("--link-protocol")
            .arg(LINK_PROTOCOL_VERSION.to_string())
            .arg("--read-bytes")
            .arg(limits.read_bytes.to_string())
            .arg("--bash-output-bytes")
            .arg(limits.bash_output_bytes.to_string())
            .arg("--bash-timeout-ms")
            .arg(limits.bash_timeout.as_millis().to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        let pid = child.id();
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("executor 没有标准输入"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("executor 没有标准输出"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("executor 没有标准错误"))?;
        let (writer, frames) = mpsc::channel::<Frame>(FRAME_QUEUE);
        let (terminate, termination) = oneshot::channel();
        let (exit, exits) = oneshot::channel::<Option<i32>>();
        let state = Arc::new(LinkState::default());
        let pings: Arc<Mutex<HashMap<u64, Pong>>> = Arc::new(Mutex::new(HashMap::new()));
        tokio::spawn(write_loop(stdin, frames));
        tokio::spawn(watch_child(
            child,
            stderr,
            exit,
            Arc::clone(&state),
            termination,
        ));
        tokio::spawn(read_loop(
            stdout,
            exits,
            Arc::clone(&state),
            Arc::clone(&pings),
        ));
        Ok(Self {
            terminate: Some(terminate),
            writer,
            state,
            pings,
            next_request: AtomicU64::new(1),
            pid,
        })
    }

    /// executor 子进程的 pid。测试与日志用来定位这条 link 的进程。
    #[must_use]
    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    /// 发一帧心跳并等应答，返回往返毫秒数。
    ///
    /// link 已关闭或超过 `timeout` 仍未应答时返回 `None`：心跳只用于量延迟，
    /// 断链由读侧与退出观察者判定。
    pub async fn ping(&self, seq: u64, timeout: Duration) -> Option<u64> {
        if self.closed().await.is_some() {
            return None;
        }
        let (sender, receiver) = oneshot::channel();
        self.pings
            .lock()
            .await
            .insert(seq, (Instant::now(), sender));
        if self.writer.send(Frame::Ping { seq }).await.is_err() {
            self.pings.lock().await.remove(&seq);
            return None;
        }
        let Ok(Ok(latency)) = tokio::time::timeout(timeout, receiver).await else {
            self.pings.lock().await.remove(&seq);
            return None;
        };
        Some(latency)
    }

    /// 当前断链原因。link 仍可用时返回 `None`。
    pub async fn closed(&self) -> Option<LinkClosed> {
        self.state.closed().await
    }

    /// link 是否空闲：没有在途请求。
    pub async fn is_idle(&self) -> bool {
        self.state.is_idle().await
    }

    /// 等 link 断开并返回它的原因。已经断开时立即返回。
    pub async fn wait_closed(&self) -> LinkClosed {
        let mut closed = self.state.subscribe();
        loop {
            let current = closed.borrow_and_update().clone();
            if let Some(closed) = current {
                return closed;
            }
            if closed.changed().await.is_err() {
                return LinkClosed::lost();
            }
        }
    }

    /// 在 executor 上执行一次工具调用。
    ///
    /// 取消时向 executor 发取消帧并等它的回答：executor 总是回一帧结果，
    /// 因此取消的结局仍是工具失败（`cancelled` 或被杀进程组之后的失败）。
    ///
    /// # Errors
    ///
    /// link 在请求发出前或等待结果期间断开时返回 [`LinkClosed`]；此时这次调用的结局未知，
    /// 不是工具失败。
    pub async fn request(
        &self,
        workspace: &str,
        tool: &str,
        arguments: &str,
        cancellation: &Cancellation,
    ) -> Result<ToolOutcome, LinkClosed> {
        let operation = Operation::Tool {
            workspace: workspace.to_owned(),
            tool: tool.to_owned(),
            arguments: arguments.to_owned(),
        };
        match self.exchange(operation, Some(cancellation)).await? {
            Reply::Outcome { outcome } => Ok(outcome),
            Reply::Workspace { .. } => Ok(mismatched_reply("工具调用")),
        }
    }

    /// 让 executor 校验一个绝对目录能否成为 workspace 身份。
    ///
    /// 文件系统事实只有 host 判定：agent 只保存结果，不重算边界。
    ///
    /// # Errors
    ///
    /// link 断开时返回 [`LinkClosed`]；此时这次校验没有结论。
    pub async fn check_workspace(
        &self,
        path: &str,
    ) -> Result<Result<String, WorkspaceCheckError>, LinkClosed> {
        let operation = Operation::CheckWorkspace {
            path: path.to_owned(),
        };
        match self.exchange(operation, None).await? {
            Reply::Workspace { result } => Ok(result),
            Reply::Outcome { .. } => Ok(Err(WorkspaceCheckError::Unreadable)),
        }
    }

    /// 发一帧请求并等配对的结果帧。
    async fn exchange(
        &self,
        operation: Operation,
        cancellation: Option<&Cancellation>,
    ) -> Result<Reply, LinkClosed> {
        let id = format!("req-{}", self.next_request.fetch_add(1, Ordering::SeqCst));
        let (sender, mut receiver) = oneshot::channel();
        self.state.register(id.clone(), sender).await?;
        let frame = Frame::Request {
            id: id.clone(),
            operation,
        };
        if self.writer.send(frame).await.is_err() {
            self.state.forget(&id).await;
            return Err(self.closed_or_lost().await);
        }
        if let Some(cancellation) = cancellation {
            tokio::select! {
                biased;
                () = cancellation.cancelled() => {}
                reply = &mut receiver => return reply.unwrap_or_else(|_| Err(LinkClosed::lost())),
            }
            if self
                .writer
                .send(Frame::Cancel { id: id.clone() })
                .await
                .is_err()
            {
                self.state.forget(&id).await;
                return Err(self.closed_or_lost().await);
            }
        }
        receiver.await.unwrap_or_else(|_| Err(LinkClosed::lost()))
    }

    /// 断链原因；等待者被丢弃但状态没写上时给一个无细节的版本。
    async fn closed_or_lost(&self) -> LinkClosed {
        self.closed().await.unwrap_or_else(LinkClosed::lost)
    }
}

impl Drop for LinkHandle {
    fn drop(&mut self) {
        if let Some(terminate) = self.terminate.take()
            && terminate.send(()).is_err()
        {
            tracing::debug!("退出观察者已结束，无需再结束 executor");
        }
    }
}

/// 答复种类与请求操作不符。link 仍然活着，因此按工具失败返回。
fn mismatched_reply(operation: &str) -> ToolOutcome {
    ToolOutcome::Failed {
        code: ToolErrorCode::IoError,
        message: format!("link 回答了与{operation}不匹配的答复"),
    }
}

/// 单写者：所有帧经这里串行写出，避免多任务交错半个帧。
async fn write_loop(mut stdin: ChildStdin, mut frames: mpsc::Receiver<Frame>) {
    while let Some(frame) = frames.recv().await {
        if let Err(error) = write_frame(&mut stdin, &frame).await {
            tracing::error!("写 link 帧失败: {error}");
            return;
        }
    }
}

/// 读侧：结果帧唤醒对应等待者，心跳应答写回延迟，读侧结束即视为断链。
async fn read_loop(
    stdout: ChildStdout,
    mut exits: oneshot::Receiver<Option<i32>>,
    state: Arc<LinkState>,
    pings: Arc<Mutex<HashMap<u64, Pong>>>,
) {
    let mut reader = BufReader::new(stdout);
    let reason = loop {
        match read_frame(&mut reader).await {
            Ok(Some(Frame::Result { id, reply })) => {
                state.resolve(&id, reply).await;
            }
            Ok(Some(Frame::Pong { seq })) => {
                let waited = pings.lock().await.remove(&seq);
                if let Some((sent, sender)) = waited {
                    let latency = u64::try_from(sent.elapsed().as_millis()).unwrap_or(u64::MAX);
                    if sender.send(latency).is_err() {
                        tracing::debug!(seq, "心跳应答没有等待者");
                    }
                } else {
                    tracing::debug!(seq, "收到不匹配的心跳应答");
                }
            }
            Ok(Some(frame)) => tracing::debug!(frame = ?frame, "收到不属于 agent 的帧，忽略"),
            Ok(None) => break "executor 关闭了 link 读侧".to_owned(),
            Err(error) => break format!("读取 link 帧失败: {error}"),
        }
    };
    let exit_code = wait_exit(&mut exits).await;
    let (closed, waiters) = state.close(exit_code).await;
    tracing::info!(
        reason = %reason,
        exit_code = ?exit_code,
        closed = %closed,
        "link 已断开"
    );
    for sender in waiters {
        if sender.send(Err(closed.clone())).is_err() {
            tracing::debug!("断链时的等待者已放弃");
        }
    }
}

/// 等到子进程退出码。超时或观察者已结束时不阻塞断链处理。
async fn wait_exit(exits: &mut oneshot::Receiver<Option<i32>>) -> Option<i32> {
    match tokio::time::timeout(EXIT_WAIT, exits).await {
        Ok(Ok(code)) => code,
        Ok(Err(error)) => {
            tracing::warn!("读取 executor 退出码失败: {error}");
            None
        }
        Err(_) => {
            tracing::warn!(
                timeout_ms = EXIT_WAIT.as_millis(),
                "等 executor 退出码超时，按无退出码的断链处理"
            );
            None
        }
    }
}

/// 退出观察者：等子进程结束、收集标准错误尾部，再发布退出码。
///
/// 丢弃 [`LinkHandle`] 时收到终止信号，先杀掉子进程；进程组清理由 executor 的
/// bash 工具自己负责，与本任务无关。
async fn watch_child(
    mut child: Child,
    stderr: ChildStderr,
    exit: oneshot::Sender<Option<i32>>,
    state: Arc<LinkState>,
    termination: oneshot::Receiver<()>,
) {
    let mut forward = tokio::spawn(forward_stderr(stderr, Arc::clone(&state)));
    let status = tokio::select! {
        status = child.wait() => status,
        _ = termination => {
            if let Err(error) = child.start_kill() {
                tracing::warn!("结束 executor 失败: {error}");
            }
            child.wait().await
        }
    };
    if tokio::time::timeout(STDERR_DRAIN, &mut forward)
        .await
        .is_err()
    {
        forward.abort();
    }
    let code = match &status {
        Ok(status) => status.code(),
        Err(error) => {
            tracing::warn!("等待 executor 退出失败: {error}");
            None
        }
    };
    if exit.send(code).is_err() {
        tracing::debug!("退出码没有接收者");
    }
}

/// executor 的日志只写标准错误，这里并入 agent 的日志并留下最后几行供断链说明。
async fn forward_stderr(stderr: ChildStderr, state: Arc<LinkState>) {
    let mut lines = BufReader::new(stderr).lines();
    loop {
        match lines.next_line().await {
            Ok(Some(line)) => {
                tracing::info!(target: "executor", "{line}");
                let mut tail = state.stderr_tail.lock().await;
                if tail.len() == STDERR_TAIL_LINES {
                    drop(tail.pop_front());
                }
                tail.push_back(line);
            }
            Ok(None) => return,
            Err(error) => {
                tracing::warn!("读取 executor 标准错误失败: {error}");
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::LinkClosed;

    #[test]
    fn exit_code_three_is_a_protocol_mismatch() {
        let closed = LinkClosed {
            exit_code: Some(3),
            stderr_tail: "帧协议版本不符".to_owned(),
        };
        assert!(closed.protocol_mismatch(), "退出码 3 表示版本不符");
        let message = closed.to_string();
        assert!(message.contains("退出码 3"), "{message}");
        assert!(message.contains("帧协议版本不符"), "{message}");
    }

    #[test]
    fn other_endings_are_not_protocol_mismatches() {
        let killed = LinkClosed {
            exit_code: None,
            stderr_tail: String::new(),
        };
        assert!(!killed.protocol_mismatch(), "被信号终止不是版本不符");
        let failed = LinkClosed {
            exit_code: Some(1),
            stderr_tail: String::new(),
        };
        assert!(!failed.protocol_mismatch(), "普通非零退出不是版本不符");
        assert_eq!(killed.to_string(), "link 已断开");
        let unavailable = LinkClosed::unavailable("host 已停用".to_owned());
        assert!(unavailable.to_string().contains("host 已停用"));
    }
}
