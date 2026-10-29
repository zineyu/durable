//! agent 二进制库：路由装配、监听地址与类型导出入口。
//! run 执行在 `engine`，host 的运行状态在 `hosts`，link 与工具契约在 `link`，
//! 进程启动在 `main`。

use std::fmt;
use std::net::SocketAddr;
use std::path::Path;

use ts_rs::TS;

pub mod protocol;

mod api;
mod config;
mod engine;
mod error;
mod hosts;
mod link;
mod provider;
mod sse;

pub use api::{AgentState, router, serve};
pub use config::{
    CONTEXT_CHAR_LIMIT, ConfigError, EngineConfig, MAX_STEPS, MODEL_API_KEY, MODEL_BASE_URL,
    MODEL_NAME, ModelConfig, engine_config_from_env, model_config_from_env,
};
pub use durable::link::{Operation, Reply, WorkspaceCheckError};
pub use durable::{
    Cancellation, ContentKind, ContentPart, DEFAULT_SESSION_TITLE, DeltaKind, FactEvent, HostKind,
    HostMetadata, HostName, HostNameError, HostStatus, HostUpdate, MessageBody, MessageId,
    MessageNode, MessageRole, MessageSkeleton, MessageText, MessageTextError, MessageTree,
    PartSkeleton, PartState, ProviderMeta, QueueItem, QueueItemId, RunFailure, RunFailureCode,
    RunId, RunRecord, RunStatus, SessionId, SessionIdError, SessionMetadata, SessionTitle,
    SessionTitleError, SessionTreeSnapshot, Store, StoreError, StreamEvent, SubmitMode,
    ToolDefinition, ToolErrorCode, ToolFailure, ToolLimits, ToolOutcome, ToolResultState,
    WorkspaceMetadata, WorkspacePath, WorkspacePathError,
};
pub use engine::RunEngine;
pub use error::ApiError;
pub use hosts::{HEARTBEAT_INTERVAL, HostRegistry, LOCAL_HOST};
pub use link::{
    INCOMPATIBLE_EXIT_CODE, LinkClosed, LinkHandle, NoTools, RemoteTools, ToolBox, ToolCall,
    ToolContext,
};
pub use protocol::{
    BootstrapResponse, CreateSessionRequest, EditMessageRequest, ErrorCode, ErrorResponse,
    HealthResponse, HealthStatus, HostSnapshot, MessageBodiesRequest, MessageBodiesResponse,
    PROTOCOL_VERSION, RegisterHostRequest, RegisterWorkspaceRequest, RemoveWorkspaceRequest,
    RenameSessionRequest, SubmitMessageRequest, SwitchActiveLeafRequest, UpdateHostRequest,
};
pub use provider::{
    ChatChunk, ChatMessage, ChatRole, ChatStream, ChatToolCall, ModelProvider, OpenAiProvider,
    ProviderError,
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

/// 解析监听地址，并确认它是回环地址。
///
/// # Errors
///
/// - [`ListenAddressError::Invalid`]：字符串无法解析为 socket 地址。
/// - [`ListenAddressError::NotLoopback`]：解析出的地址不是回环地址。
pub fn parse_listen_address(raw: &str) -> Result<SocketAddr, ListenAddressError> {
    let address: SocketAddr = raw
        .parse()
        .map_err(|_| ListenAddressError::Invalid(raw.to_owned()))?;
    if !address.ip().is_loopback() {
        return Err(ListenAddressError::NotLoopback(raw.to_owned()));
    }
    Ok(address)
}

/// 把全部协议类型导出到给定目录。
///
/// # Errors
///
/// - `std::io::Error`：输出目录无法创建。
/// - `ts_rs::ExportError`：任一协议类型导出失败。
pub fn export_types(directory: &Path) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(directory)?;
    let config = ts_rs::Config::new().with_out_dir(directory);
    HealthResponse::export_all(&config)?;
    HealthStatus::export_all(&config)?;
    HostName::export_all(&config)?;
    HostKind::export_all(&config)?;
    HostMetadata::export_all(&config)?;
    HostStatus::export_all(&config)?;
    HostSnapshot::export_all(&config)?;
    RegisterHostRequest::export_all(&config)?;
    UpdateHostRequest::export_all(&config)?;
    WorkspacePath::export_all(&config)?;
    WorkspaceMetadata::export_all(&config)?;
    SessionId::export_all(&config)?;
    SessionTitle::export_all(&config)?;
    SessionMetadata::export_all(&config)?;
    BootstrapResponse::export_all(&config)?;
    MessageRole::export_all(&config)?;
    MessageText::export_all(&config)?;
    ContentKind::export_all(&config)?;
    PartState::export_all(&config)?;
    ProviderMeta::export_all(&config)?;
    ToolOutcome::export_all(&config)?;
    ToolResultState::export_all(&config)?;
    ContentPart::export_all(&config)?;
    PartSkeleton::export_all(&config)?;
    MessageSkeleton::export_all(&config)?;
    MessageBody::export_all(&config)?;
    MessageBodiesRequest::export_all(&config)?;
    MessageBodiesResponse::export_all(&config)?;
    ToolErrorCode::export_all(&config)?;
    QueueItem::export_all(&config)?;
    RunStatus::export_all(&config)?;
    RunFailureCode::export_all(&config)?;
    RunFailure::export_all(&config)?;
    RunRecord::export_all(&config)?;
    SessionTreeSnapshot::export_all(&config)?;
    FactEvent::export_all(&config)?;
    StreamEvent::export_all(&config)?;
    ErrorCode::export_all(&config)?;
    ErrorResponse::export_all(&config)?;
    RegisterWorkspaceRequest::export_all(&config)?;
    RemoveWorkspaceRequest::export_all(&config)?;
    CreateSessionRequest::export_all(&config)?;
    RenameSessionRequest::export_all(&config)?;
    SubmitMessageRequest::export_all(&config)?;
    SubmitMode::export_all(&config)?;
    EditMessageRequest::export_all(&config)?;
    SwitchActiveLeafRequest::export_all(&config)?;
    Ok(())
}
