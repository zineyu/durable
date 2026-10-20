use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use runtime::dirs;
use runtime::logging::{LogConfig, init};
use tracing::info;

#[derive(Debug, Parser)]
struct Arguments {
    /// HTTP 监听地址。
    #[arg(long, default_value = agent::DEFAULT_LISTEN)]
    listen: String,
    /// SQLite 路径。省略时使用标准数据目录。
    #[arg(long)]
    database: Option<PathBuf>,
    /// 日志目录。省略时使用标准日志目录。
    #[arg(long = "log-dir")]
    log_dir: Option<PathBuf>,
    /// 启动时注册的 workspace 路径，可重复。
    #[arg(long = "workspace")]
    workspaces: Vec<PathBuf>,
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
    match run(arguments, prepared).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!("启动失败: {error}");
            ExitCode::FAILURE
        }
    }
}

fn prepare(arguments: &Arguments) -> Result<Prepared, Box<dyn std::error::Error>> {
    let address = agent::parse_listen_address(&arguments.listen)?;
    let database = match &arguments.database {
        Some(path) => path.clone(),
        None => dirs::app_dirs()?.data.join(agent::DEFAULT_DATABASE_FILE),
    };
    let log_directory = match &arguments.log_dir {
        Some(path) => path.clone(),
        None => dirs::app_dirs()?.logs,
    };
    Ok(Prepared {
        address,
        database,
        log: LogConfig::new(log_directory, agent::LOG_FILE_PREFIX),
    })
}

async fn run(arguments: Arguments, prepared: Prepared) -> Result<(), Box<dyn std::error::Error>> {
    let model = agent::model_config_from_env()?;
    let engine = agent::engine_config_from_env()?;
    let workspaces = resolve_workspaces(&arguments.workspaces)?;
    let store = agent::Store::open(&prepared.database).await?;
    store.register_initial_workspaces(&workspaces).await?;
    let provider = std::sync::Arc::new(agent::OpenAiProvider::new(&model));
    let model_name = model.model_name.clone();
    let state = agent::AgentState::start(
        store,
        model_name,
        provider,
        std::sync::Arc::new(agent::ToolRegistry::default()),
        engine,
    )
    .await?;
    info!(
        database = %prepared.database.display(),
        log_directory = %prepared.log.directory.display(),
        log_filter = %prepared.log.filter,
        workspaces = workspaces.len(),
        model = %model.model_name,
        "agent 启动"
    );
    let listener = tokio::net::TcpListener::bind(prepared.address).await?;
    let bound = listener.local_addr()?;
    println!("agent listening on http://{bound}");
    info!(listen = %bound, "agent 开始监听");
    agent::serve(listener, state).await?;
    Ok(())
}

/// 把启动参数中的 workspace 路径解析为规范化身份。
///
/// 失败：路径不是 UTF-8、不是绝对路径、不存在或不是目录。
fn resolve_workspaces(
    paths: &[PathBuf],
) -> Result<Vec<agent::WorkspacePath>, agent::WorkspacePathError> {
    paths
        .iter()
        .map(|path| {
            let raw = path
                .to_str()
                .ok_or_else(|| agent::WorkspacePathError::NotUtf8(path.display().to_string()))?;
            agent::WorkspacePath::resolve(raw)
        })
        .collect()
}
