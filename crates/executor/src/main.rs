//! executor 进程入口：读 link 帧、执行工具、把结果写回标准输出。
//! 工具实现在 `tools`，帧定义在 `durable::link`，链路驱动在 `crates/agent/src/link`。

use std::collections::{HashMap, VecDeque};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use durable::Cancellation;
use durable::ToolLimits;
use durable::link::{Frame, LINK_PROTOCOL_VERSION, Operation, Reply, read_frame, write_frame};
use tokio::io::BufReader;
use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinSet;
use tracing_subscriber::EnvFilter;

mod tools;

/// 帧协议版本不符时的退出码。调用方据此把 host 置为 `incompatible`。
const INCOMPATIBLE_EXIT_CODE: u8 = 3;

/// 标准输入 EOF 之后等待在途请求自行收尾的时间。
const DRAIN_TIMEOUT: Duration = Duration::from_secs(2);

/// 待写帧队列容量。写侧只有一个任务，容量只用于吸收突发。
const FRAME_QUEUE: usize = 64;

/// 退出日志里保留的最近帧数。
const RECENT_FRAMES: usize = 8;

#[derive(Debug, Parser)]
struct Arguments {
    /// link 帧协议版本。
    #[arg(long = "link-protocol")]
    link_protocol: u32,
    /// `read_file` 的字节上限。
    #[arg(long = "read-bytes")]
    read_bytes: Option<usize>,
    /// `bash` 输出的字节上限。
    #[arg(long = "bash-output-bytes")]
    bash_output_bytes: Option<usize>,
    /// `bash` 的超时毫秒数。
    #[arg(long = "bash-timeout-ms")]
    bash_timeout_ms: Option<u64>,
}

impl Arguments {
    fn limits(&self) -> ToolLimits {
        let defaults = ToolLimits::default();
        ToolLimits {
            read_bytes: self.read_bytes.unwrap_or(defaults.read_bytes),
            bash_output_bytes: self.bash_output_bytes.unwrap_or(defaults.bash_output_bytes),
            bash_timeout: self
                .bash_timeout_ms
                .map_or(defaults.bash_timeout, Duration::from_millis),
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let arguments = Arguments::parse();
    init_logging();
    if arguments.link_protocol != LINK_PROTOCOL_VERSION {
        tracing::error!(
            expected = LINK_PROTOCOL_VERSION,
            received = arguments.link_protocol,
            "link 帧协议版本不符"
        );
        return ExitCode::from(INCOMPATIBLE_EXIT_CODE);
    }
    serve(arguments.limits()).await;
    ExitCode::SUCCESS
}

/// 日志只写标准错误，由 agent 并入自己的日志。host 上不留日志文件。
fn init_logging() {
    let filter = EnvFilter::try_from_env("DURABLE_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_target(false)
        .init();
}

/// 一个 host 上的唯一任务：读帧、派发工具、按 id 收取消、在 EOF 时收尾。
async fn serve(limits: ToolLimits) {
    let (sender, receiver) = mpsc::channel::<Frame>(FRAME_QUEUE);
    let writer = tokio::spawn(write_loop(receiver));
    let active: Arc<Mutex<HashMap<String, Cancellation>>> = Arc::new(Mutex::new(HashMap::new()));
    let mut tasks: JoinSet<()> = JoinSet::new();
    let mut input = BufReader::new(tokio::io::stdin());
    let mut recent: VecDeque<String> = VecDeque::new();
    let reason = loop {
        match read_frame(&mut input).await {
            Ok(Some(frame)) => {
                remember(&mut recent, describe(&frame));
                match frame {
                    Frame::Request { id, operation } => {
                        let cancellation = Cancellation::new();
                        active.lock().await.insert(id.clone(), cancellation.clone());
                        let sender = sender.clone();
                        let active = Arc::clone(&active);
                        tasks.spawn(async move {
                            let reply = match operation {
                                Operation::Tool {
                                    workspace,
                                    tool,
                                    arguments,
                                } => Reply::Outcome {
                                    outcome: tools::execute(
                                        &tool,
                                        &workspace,
                                        &arguments,
                                        &cancellation,
                                        &limits,
                                    )
                                    .await,
                                },
                                Operation::CheckWorkspace { path } => Reply::Workspace {
                                    result: tools::check_workspace(&path),
                                },
                            };
                            active.lock().await.remove(&id);
                            if sender.send(Frame::Result { id, reply }).await.is_err() {
                                tracing::warn!("link 写侧已关闭，请求结果被丢弃");
                            }
                        });
                    }
                    Frame::Cancel { id } => {
                        let cancellation = active.lock().await.get(&id).cloned();
                        if let Some(cancellation) = cancellation {
                            cancellation.cancel();
                        } else {
                            tracing::debug!(id = %id, "取消没有匹配的在途请求");
                        }
                    }
                    Frame::Ping { seq } => {
                        if sender.send(Frame::Pong { seq }).await.is_err() {
                            tracing::warn!("link 写侧已关闭，心跳应答被丢弃");
                        }
                    }
                    Frame::Result { .. } | Frame::Pong { .. } => {
                        tracing::debug!("收到不属于 executor 的帧，忽略");
                    }
                }
            }
            Ok(None) => break "标准输入 EOF".to_owned(),
            Err(error) => break format!("读取 link 帧失败: {error}"),
        }
    };
    let active_count = active.lock().await.len();
    tracing::info!(
        reason = %reason,
        active = active_count,
        recent = ?recent,
        "link 读侧结束，取消在途请求后退出"
    );
    cancel_all(&active).await;
    drop(sender);
    drain(&mut tasks).await;
    if let Err(error) = writer.await {
        tracing::warn!("写侧任务结束异常: {error}");
    }
}

/// 一帧的简短描述，用于退出日志。不打印参数正文，避免日志被工具输入撑大。
fn describe(frame: &Frame) -> String {
    match frame {
        Frame::Request { id, operation } => match operation {
            Operation::Tool { tool, .. } => format!("request {id} {tool}"),
            Operation::CheckWorkspace { .. } => format!("request {id} check_workspace"),
        },
        Frame::Result { id, .. } => format!("result {id}"),
        Frame::Cancel { id } => format!("cancel {id}"),
        Frame::Ping { seq } => format!("ping {seq}"),
        Frame::Pong { seq } => format!("pong {seq}"),
    }
}

/// 保留最近 [`RECENT_FRAMES`] 帧。
fn remember(recent: &mut VecDeque<String>, line: String) {
    if recent.len() == RECENT_FRAMES {
        drop(recent.pop_front());
    }
    recent.push_back(line);
}

/// 读侧结束后取消全部在途请求，并限时等待它们回帧。
async fn drain(tasks: &mut JoinSet<()>) {
    let finished = tokio::time::timeout(DRAIN_TIMEOUT, async {
        while tasks.join_next().await.is_some() {}
    });
    if finished.await.is_err() {
        tracing::warn!(
            timeout_ms = DRAIN_TIMEOUT.as_millis(),
            "在途工具未在时限内结束，丢弃"
        );
        tasks.shutdown().await;
    }
}

async fn cancel_all(active: &Mutex<HashMap<String, Cancellation>>) {
    let active = active.lock().await;
    for cancellation in active.values() {
        cancellation.cancel();
    }
}

/// 单写者：所有帧经这里串行写出，避免多任务交错半个帧。
async fn write_loop(mut frames: mpsc::Receiver<Frame>) {
    let mut output = tokio::io::stdout();
    while let Some(frame) = frames.recv().await {
        if let Err(error) = write_frame(&mut output, &frame).await {
            tracing::error!("写 link 帧失败: {error}");
            return;
        }
    }
}
