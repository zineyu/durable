//! link 的 agent 侧：启动 executor 子进程，按请求 id 多路复用帧。
//! `ToolBox` 实现在 `tools`；帧与工具体现在 `crates/core` 与 `crates/executor`。
//!
//! 规格 user story R-35：同一 host 上的多个 session 共用一条 link，请求按 id 配对。

use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

use durable::link::{Frame, LINK_PROTOCOL_VERSION, read_frame, write_frame};
use durable::{Cancellation, ToolErrorCode, ToolLimits, ToolOutcome};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::sync::{Mutex, mpsc, oneshot};

mod tools;

pub use tools::{NoTools, RemoteTools, ToolBox, ToolCall, ToolContext};

/// link 断开时给在途调用与后续调用的结局。
const LINK_CLOSED: &str = "link 已断开";

/// 待写帧队列容量。写侧只有一个任务，容量只用于吸收突发。
const FRAME_QUEUE: usize = 64;

/// 一条本地 link：executor 子进程、写侧任务、读侧任务与在途请求表。
///
/// 一个 host 只有一条 link，多个 session 共用它；请求按 id 多路复用。
pub struct LinkHandle {
    /// 子进程句柄。丢弃本结构即以 `kill_on_drop` 结束 executor。
    _child: tokio::process::Child,
    writer: mpsc::Sender<Frame>,
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<ToolOutcome>>>>,
    pings: Arc<Mutex<HashMap<u64, Instant>>>,
    closed: Arc<AtomicBool>,
    next_request: AtomicU64,
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
        tokio::spawn(write_loop(stdin, frames));
        let pending: Arc<Mutex<HashMap<String, oneshot::Sender<ToolOutcome>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let pings: Arc<Mutex<HashMap<u64, Instant>>> = Arc::new(Mutex::new(HashMap::new()));
        let closed = Arc::new(AtomicBool::new(false));
        tokio::spawn(read_loop(
            stdout,
            Arc::clone(&pending),
            Arc::clone(&pings),
            Arc::clone(&closed),
        ));
        tokio::spawn(forward_stderr(stderr));
        Ok(Self {
            _child: child,
            writer,
            pending,
            pings,
            closed,
            next_request: AtomicU64::new(1),
        })
    }

    /// 发一帧心跳。应答到达时读侧以 debug 记录往返毫秒数。
    pub async fn ping(&self, seq: u64) {
        self.pings.lock().await.insert(seq, Instant::now());
        if self.writer.send(Frame::Ping { seq }).await.is_err() {
            self.pings.lock().await.remove(&seq);
            tracing::debug!(seq, "心跳未能发出：link 已关闭");
        }
    }

    /// 在 executor 上执行一次工具调用。
    ///
    /// 取消时向 executor 发取消帧并等它的回答：executor 总是回一帧结果，
    /// 因此结局仍是工具失败（`cancelled` 或被杀进程组之后的失败），不是未知。
    pub async fn request(
        &self,
        workspace: &str,
        tool: &str,
        arguments: &str,
        cancellation: &Cancellation,
    ) -> ToolOutcome {
        if self.closed.load(Ordering::SeqCst) {
            return link_closed();
        }
        let id = format!("req-{}", self.next_request.fetch_add(1, Ordering::SeqCst));
        let (sender, mut receiver) = oneshot::channel();
        self.pending.lock().await.insert(id.clone(), sender);
        let frame = Frame::Request {
            id: id.clone(),
            workspace: workspace.to_owned(),
            tool: tool.to_owned(),
            arguments: arguments.to_owned(),
        };
        if self.writer.send(frame).await.is_err() {
            self.pending.lock().await.remove(&id);
            return link_closed();
        }
        tokio::select! {
            biased;
            () = cancellation.cancelled() => {}
            outcome = &mut receiver => return outcome.unwrap_or_else(|_| link_closed()),
        }
        if self
            .writer
            .send(Frame::Cancel { id: id.clone() })
            .await
            .is_err()
        {
            self.pending.lock().await.remove(&id);
            return link_closed();
        }
        receiver.await.unwrap_or_else(|_| link_closed())
    }
}

fn link_closed() -> ToolOutcome {
    ToolOutcome::Failed {
        code: ToolErrorCode::IoError,
        message: LINK_CLOSED.to_owned(),
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

/// 读侧：结果帧唤醒对应等待者，心跳记延迟，读侧结束即视为断链。
async fn read_loop(
    stdout: ChildStdout,
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<ToolOutcome>>>>,
    pings: Arc<Mutex<HashMap<u64, Instant>>>,
    closed: Arc<AtomicBool>,
) {
    let mut reader = BufReader::new(stdout);
    loop {
        match read_frame(&mut reader).await {
            Ok(Some(Frame::Result { id, outcome })) => {
                let waiter = pending.lock().await.remove(&id);
                if let Some(sender) = waiter {
                    if sender.send(outcome).is_err() {
                        tracing::debug!(id = %id, "工具结果没有等待者");
                    }
                } else {
                    tracing::debug!(id = %id, "收到不匹配的工具结果");
                }
            }
            Ok(Some(Frame::Pong { seq })) => {
                let sent = pings.lock().await.remove(&seq);
                if let Some(sent) = sent {
                    tracing::debug!(seq, latency_ms = sent.elapsed().as_millis(), "收到心跳应答");
                } else {
                    tracing::debug!(seq, "收到不匹配的心跳应答");
                }
            }
            Ok(Some(frame)) => tracing::debug!(frame = ?frame, "收到不属于 agent 的帧，忽略"),
            Ok(None) => {
                tracing::info!("link 读侧结束");
                break;
            }
            Err(error) => {
                tracing::error!("读取 link 帧失败: {error}");
                break;
            }
        }
    }
    closed.store(true, Ordering::SeqCst);
    fail_pending(&pending).await;
}

/// 断链后所有在途调用立即以 `io_error` 结束，调用方据此写「结果未知」。
async fn fail_pending(pending: &Mutex<HashMap<String, oneshot::Sender<ToolOutcome>>>) {
    let waiters: Vec<oneshot::Sender<ToolOutcome>> = pending
        .lock()
        .await
        .drain()
        .map(|(_, sender)| sender)
        .collect();
    for sender in waiters {
        if sender.send(link_closed()).is_err() {
            tracing::debug!("断链时的等待者已放弃");
        }
    }
}

/// executor 的日志只写标准错误，这里并入 agent 的日志。
async fn forward_stderr(stderr: ChildStderr) {
    let mut lines = BufReader::new(stderr).lines();
    loop {
        match lines.next_line().await {
            Ok(Some(line)) => tracing::info!(target: "executor", "{line}"),
            Ok(None) => return,
            Err(error) => {
                tracing::warn!("读取 executor 标准错误失败: {error}");
                return;
            }
        }
    }
}
