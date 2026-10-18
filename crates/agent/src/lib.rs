use std::fmt;
use std::net::SocketAddr;

use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const DEFAULT_LISTEN: &str = "127.0.0.1:8081";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    Ok,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct HealthResponse {
    pub status: HealthStatus,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListenAddressError {
    Invalid(String),
    NotLoopback(String),
}

impl fmt::Display for ListenAddressError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(raw) => write!(formatter, "无法解析监听地址: {raw}"),
            Self::NotLoopback(raw) => write!(formatter, "监听地址必须是回环地址: {raw}"),
        }
    }
}

impl std::error::Error for ListenAddressError {}

pub fn parse_listen_address(raw: &str) -> Result<SocketAddr, ListenAddressError> {
    let address: SocketAddr = raw
        .parse()
        .map_err(|_| ListenAddressError::Invalid(raw.to_owned()))?;
    if !address.ip().is_loopback() {
        return Err(ListenAddressError::NotLoopback(raw.to_owned()));
    }
    Ok(address)
}

pub fn router() -> Router {
    Router::new().route("/api/health", get(health))
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: HealthStatus::Ok,
        version: env!("CARGO_PKG_VERSION").to_owned(),
    })
}

pub async fn serve(listener: tokio::net::TcpListener) -> std::io::Result<()> {
    axum::serve(listener, router()).await
}

pub fn export_types(directory: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(directory)?;
    let config = ts_rs::Config::new().with_out_dir(directory);
    HealthResponse::export_all(&config)?;
    Ok(())
}
