use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use clap::Parser;

#[derive(Debug, Parser)]
struct Arguments {
    #[arg(long, default_value = hub::DEFAULT_LISTEN)]
    listen: String,
    #[arg(long)]
    database: PathBuf,
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
    let store = Arc::new(durable::Store::open(&arguments.database).await?);
    let listener = tokio::net::TcpListener::bind(address).await?;
    println!("hub listening on http://{address}");
    hub::serve(listener, store).await?;
    Ok(())
}
