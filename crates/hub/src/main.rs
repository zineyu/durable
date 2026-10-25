//! hub 进程入口：解析参数、准备目录与日志、启动 HTTP 服务。

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use clap::Parser;
use runtime::dirs;
use runtime::logging::{LogConfig, init};
use tracing::info;

#[derive(Debug, Parser)]
struct Arguments {
    /// HTTP 监听地址。
    #[arg(long, default_value = hub::DEFAULT_LISTEN)]
    listen: String,
    /// SQLite 路径。省略时使用标准数据目录。
    #[arg(long)]
    database: Option<PathBuf>,
    /// 日志目录。省略时使用标准日志目录。
    #[arg(long = "log-dir")]
    log_dir: Option<PathBuf>,
    /// 广播通道容量。每个客户端最多落后这么多条事件。
    #[arg(long = "broadcast-capacity", default_value_t = hub::DEFAULT_BROADCAST_CAPACITY)]
    broadcast_capacity: usize,
    /// 回放窗口条数。游标落在窗口之外时客户端收到 resync。
    #[arg(long = "replay-window", default_value_t = hub::DEFAULT_REPLAY_WINDOW)]
    replay_window: usize,
}

/// 已确定的启动路径与日志参数。
struct Prepared {
    address: SocketAddr,
    database: PathBuf,
    log: LogConfig,
    options: hub::HubOptions,
}

#[tokio::main]
async fn main() -> ExitCode {
    let arguments = Arguments::parse();
    let prepared = match prepare(&arguments) {
        Ok(prepared) => prepared,
        Err(error) => return startup_failure(error),
    };
    let _log = match init(&prepared.log) {
        Ok(guard) => guard,
        Err(error) => return startup_failure(error),
    };
    match run(prepared).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!("启动失败: {error}");
            ExitCode::FAILURE
        }
    }
}

/// 在日志可用之前报告启动错误。
#[expect(
    clippy::print_stderr,
    reason = "启动路径上日志尚未可用（未初始化或初始化失败），只能写标准错误。"
)]
fn startup_failure(error: impl std::fmt::Display) -> ExitCode {
    eprintln!("{error}");
    ExitCode::FAILURE
}

fn prepare(arguments: &Arguments) -> Result<Prepared, Box<dyn std::error::Error>> {
    let address = hub::parse_listen_address(&arguments.listen)?;
    if arguments.broadcast_capacity == 0 || arguments.replay_window == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "--broadcast-capacity 与 --replay-window 必须大于 0",
        )
        .into());
    }
    let paths = dirs::startup_paths(
        arguments.database.as_deref(),
        arguments.log_dir.as_deref(),
        hub::DEFAULT_DATABASE_FILE,
    )?;
    Ok(Prepared {
        address,
        database: paths.database,
        log: LogConfig::new(paths.log_directory, hub::LOG_FILE_PREFIX),
        options: hub::HubOptions {
            broadcast_capacity: arguments.broadcast_capacity,
            replay_window: arguments.replay_window,
            ..hub::HubOptions::default()
        },
    })
}

#[expect(
    clippy::print_stdout,
    reason = "这一行是启动测试与 client e2e 约定的就绪信号，必须写标准输出。"
)]
async fn run(prepared: Prepared) -> Result<(), Box<dyn std::error::Error>> {
    let store = Arc::new(durable::Store::open(&prepared.database).await?);
    let listener = tokio::net::TcpListener::bind(prepared.address).await?;
    let bound = listener.local_addr()?;
    info!(
        database = %prepared.database.display(),
        log_directory = %prepared.log.directory.display(),
        log_filter = %prepared.log.filter,
        broadcast_capacity = prepared.options.broadcast_capacity,
        replay_window = prepared.options.replay_window,
        "hub 启动"
    );
    println!("hub listening on http://{bound}");
    info!(listen = %bound, "hub 开始监听");
    hub::serve_with(listener, store, prepared.options).await?;
    Ok(())
}
