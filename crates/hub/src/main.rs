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
}

/// 已确定的启动路径与日志参数。
struct Prepared {
    address: SocketAddr,
    database: PathBuf,
    log: LogConfig,
}

#[tokio::main]
async fn main() -> ExitCode {
    let arguments = Arguments::parse();
    let prepared = match prepare(&arguments) {
        Ok(prepared) => prepared,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let _log = match init(&prepared.log) {
        Ok(guard) => guard,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    match run(prepared).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!("启动失败: {error}");
            ExitCode::FAILURE
        }
    }
}

fn prepare(arguments: &Arguments) -> Result<Prepared, Box<dyn std::error::Error>> {
    let address = hub::parse_listen_address(&arguments.listen)?;
    let paths = dirs::startup_paths(
        arguments.database.as_deref(),
        arguments.log_dir.as_deref(),
        hub::DEFAULT_DATABASE_FILE,
    )?;
    Ok(Prepared {
        address,
        database: paths.database,
        log: LogConfig::new(paths.log_directory, hub::LOG_FILE_PREFIX),
    })
}

async fn run(prepared: Prepared) -> Result<(), Box<dyn std::error::Error>> {
    let store = Arc::new(durable::Store::open(&prepared.database).await?);
    let listener = tokio::net::TcpListener::bind(prepared.address).await?;
    let bound = listener.local_addr()?;
    info!(
        database = %prepared.database.display(),
        log_directory = %prepared.log.directory.display(),
        log_filter = %prepared.log.filter,
        "hub 启动"
    );
    println!("hub listening on http://{bound}");
    info!(listen = %bound, "hub 开始监听");
    hub::serve(listener, store).await?;
    Ok(())
}
