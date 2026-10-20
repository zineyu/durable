use std::process::ExitCode;

use clap::Parser;

#[derive(Debug, Parser)]
struct Arguments {
    #[arg(long, default_value = hub::DEFAULT_LISTEN)]
    listen: String,
    #[arg(long, default_value = hub::DEFAULT_AGENT_URL)]
    agent_url: String,
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
    let address = hub::parse_listen_address(&arguments.listen)?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    println!(
        "hub listening on http://{address}, proxying /api to {}",
        arguments.agent_url
    );
    hub::serve(listener, arguments.agent_url).await?;
    Ok(())
}
