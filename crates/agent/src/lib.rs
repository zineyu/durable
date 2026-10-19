use std::fmt;
use std::net::SocketAddr;
use std::path::Path;

use ts_rs::TS;

mod api;
mod config;
mod error;
mod protocol;
mod store;

pub use api::{router, serve, AgentState};
pub use config::{
    model_config_from_env, ConfigError, ModelConfig, MODEL_API_KEY, MODEL_BASE_URL, MODEL_NAME,
};
pub use error::{ApiError, StoreError};
pub use protocol::{
    BootstrapResponse, CreateSessionRequest, ErrorCode, ErrorResponse, FactEvent, HealthResponse,
    HealthStatus, MessageNode, MessageRole, RegisterWorkspaceRequest, RemoveWorkspaceRequest,
    RenameSessionRequest, SessionId, SessionIdError, SessionMetadata, SessionTitle,
    SessionTitleError, SessionTree, WorkspaceMetadata, WorkspacePath, WorkspacePathError,
    DEFAULT_SESSION_TITLE,
};
pub use store::{Snapshot, Store};

pub const DEFAULT_LISTEN: &str = "127.0.0.1:8081";

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

pub fn export_types(directory: &Path) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(directory)?;
    let config = ts_rs::Config::new().with_out_dir(directory);
    HealthResponse::export_all(&config)?;
    HealthStatus::export_all(&config)?;
    WorkspacePath::export_all(&config)?;
    WorkspaceMetadata::export_all(&config)?;
    SessionId::export_all(&config)?;
    SessionTitle::export_all(&config)?;
    SessionMetadata::export_all(&config)?;
    BootstrapResponse::export_all(&config)?;
    MessageRole::export_all(&config)?;
    MessageNode::export_all(&config)?;
    SessionTree::export_all(&config)?;
    FactEvent::export_all(&config)?;
    ErrorCode::export_all(&config)?;
    ErrorResponse::export_all(&config)?;
    RegisterWorkspaceRequest::export_all(&config)?;
    RemoveWorkspaceRequest::export_all(&config)?;
    CreateSessionRequest::export_all(&config)?;
    RenameSessionRequest::export_all(&config)?;
    Ok(())
}
