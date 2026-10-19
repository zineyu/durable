use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

#[derive(Debug, Parser)]
struct Arguments {
    #[arg(long, default_value = agent::DEFAULT_LISTEN)]
    listen: String,
    #[arg(long)]
    database: PathBuf,
    #[arg(long = "workspace")]
    workspaces: Vec<PathBuf>,
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = Arguments::parse();
    let model = agent::model_config_from_env()?;
    let engine = agent::engine_config_from_env()?;
    let address = agent::parse_listen_address(&arguments.listen)?;
    let workspaces = resolve_workspaces(&arguments.workspaces)?;
    let store = agent::Store::open(&arguments.database).await?;
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
    let listener = tokio::net::TcpListener::bind(address).await?;
    let bound = listener.local_addr()?;
    println!("agent listening on http://{bound}");
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
