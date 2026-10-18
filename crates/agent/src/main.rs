use std::process::ExitCode;

use clap::Parser;

#[derive(Debug, Parser)]
struct Arguments {
    #[arg(long, default_value = agent::DEFAULT_LISTEN)]
    listen: String,
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
    let address = agent::parse_listen_address(&arguments.listen)?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    println!("agent listening on http://{address}");
    agent::serve(listener).await?;
    Ok(())
}
