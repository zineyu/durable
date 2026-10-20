use std::fmt;
use std::net::SocketAddr;
use std::path::Path;

use ts_rs::TS;

pub mod protocol;

mod api;
mod cancel;
mod config;
mod engine;
mod error;
mod provider;
mod sse;
mod tools;

pub use api::{AgentState, router, serve};
pub use cancel::Cancellation;
pub use config::{
    CONTEXT_CHAR_LIMIT, ConfigError, EngineConfig, MAX_STEPS, MODEL_API_KEY, MODEL_BASE_URL,
    MODEL_NAME, ModelConfig, engine_config_from_env, model_config_from_env,
};
pub use durable::{
    DEFAULT_SESSION_TITLE, FactEvent, MessageContent, MessageId, MessageNode, MessageRole,
    MessageText, MessageTextError, QueueItem, QueueItemId, RunFailure, RunFailureCode, RunId,
    RunRecord, RunStatus, SessionId, SessionIdError, SessionMetadata, SessionTitle,
    SessionTitleError, SessionTree, Store, StoreError, StreamEvent, ToolCallRecord, ToolErrorCode,
    ToolOutcome, WorkspaceMetadata, WorkspacePath, WorkspacePathError,
};
pub use engine::RunEngine;
pub use error::ApiError;
pub use protocol::{
    BootstrapResponse, CreateSessionRequest, EditMessageRequest, ErrorCode, ErrorResponse,
    HealthResponse, HealthStatus, PROTOCOL_VERSION, RegisterWorkspaceRequest,
    RemoveWorkspaceRequest, RenameSessionRequest, SubmitMessageRequest, SwitchActiveLeafRequest,
};
pub use provider::{
    ChatChunk, ChatMessage, ChatRole, ChatStream, ChatToolCall, ModelProvider, OpenAiProvider,
    ProviderError,
};
pub use tools::{
    NoTools, ToolBox, ToolCall, ToolContext, ToolDefinition, ToolLimits, ToolRegistry,
};

pub const DEFAULT_LISTEN: &str = "127.0.0.1:8081";
/// 未指定 `--database` 时使用的数据库文件名。
pub const DEFAULT_DATABASE_FILE: &str = "agent.sqlite";
/// 日志文件的前缀。
pub const LOG_FILE_PREFIX: &str = "agent";

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
    MessageText::export_all(&config)?;
    MessageContent::export_all(&config)?;
    MessageNode::export_all(&config)?;
    ToolCallRecord::export_all(&config)?;
    ToolOutcome::export_all(&config)?;
    ToolErrorCode::export_all(&config)?;
    QueueItem::export_all(&config)?;
    RunStatus::export_all(&config)?;
    RunFailureCode::export_all(&config)?;
    RunFailure::export_all(&config)?;
    RunRecord::export_all(&config)?;
    SessionTree::export_all(&config)?;
    FactEvent::export_all(&config)?;
    StreamEvent::export_all(&config)?;
    ErrorCode::export_all(&config)?;
    ErrorResponse::export_all(&config)?;
    RegisterWorkspaceRequest::export_all(&config)?;
    RemoveWorkspaceRequest::export_all(&config)?;
    CreateSessionRequest::export_all(&config)?;
    RenameSessionRequest::export_all(&config)?;
    SubmitMessageRequest::export_all(&config)?;
    EditMessageRequest::export_all(&config)?;
    SwitchActiveLeafRequest::export_all(&config)?;
    Ok(())
}
