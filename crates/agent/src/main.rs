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
    let address = agent::parse_listen_address(&arguments.listen)?;
    let store = agent::Store::open(&arguments.database)?;
    store.register_initial_workspaces(&arguments.workspaces)?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    let bound = listener.local_addr()?;
    println!("agent listening on http://{bound}");
    agent::serve(listener, agent::AgentState::new(store, model.model_name)).await?;
    Ok(())
}
