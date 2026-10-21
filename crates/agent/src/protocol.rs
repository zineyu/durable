//! agent 的 HTTP 协议 DTO 与协议版本。
//! hub 自有协议在 `crates/hub/src/protocol`。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use durable::{MessageId, SessionMetadata, WorkspaceMetadata};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    Ok,
}

/// 协议版本。任何不兼容的协议改动都递增它。
pub const PROTOCOL_VERSION: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct HealthResponse {
    pub status: HealthStatus,
    pub version: String,
    pub protocol_version: u32,
}

// i64 字段是毫秒时间戳与事件 id，均小于 JS 安全整数上限；TS 侧映射为 number。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapResponse {
    pub model: String,
    #[ts(type = "number")]
    pub max_event_id: i64,
    pub workspaces: Vec<WorkspaceMetadata>,
    pub sessions: Vec<SessionMetadata>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    WorkspacePathInvalid,
    WorkspaceDuplicate,
    WorkspaceNotFound,
    WorkspaceNotEmpty,
    SessionNotFound,
    QueueItemNotFound,
    MessageNotFound,
    MessageRoleMismatch,
    RunInProgress,
    QueueNotEmpty,
    InvalidTitle,
    InvalidMessage,
    InvalidRequest,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ErrorResponse {
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RegisterWorkspaceRequest {
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RemoveWorkspaceRequest {
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct CreateSessionRequest {
    pub workspace: String,
    pub title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RenameSessionRequest {
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SubmitMessageRequest {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct EditMessageRequest {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SwitchActiveLeafRequest {
    #[ts(type = "number")]
    pub message_id: MessageId,
}
