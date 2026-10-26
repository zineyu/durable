//! agent 的 HTTP 协议 DTO 与协议版本。
//! hub 自有协议在 `crates/hub/src/protocol`。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use durable::{MessageBody, MessageId, SessionMetadata, SubmitMode, WorkspaceMetadata};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    Ok,
}

/// 协议版本。任何不兼容的协议改动都递增它。
///
/// 4：提交消息必须写明 `mode`。不带 `mode` 的旧客户端被拒绝；旧节点忽略该字段，会把插话静默排队。
/// 5：事实事件的载荷不再带 `session`，session 视图移到事件信封。旧 hub 读不到 `sessionId`，
///    新 hub 读不到旧载荷的 `session`，两个方向都不兼容。
/// 6：`runFailureCode` 新增 `storage_failed`，`errorCode` 新增 `node_name_taken` 与
///    `node_not_found`。客户端的失败标签表是穷尽 `Record`，旧客户端遇未知键会渲染出
///    `undefined` 前缀的失败说明。
pub const PROTOCOL_VERSION: u32 = 6;

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
    NodeNameTaken,
    NodeNotFound,
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
    /// 必填。省略时请求被拒绝。
    pub mode: SubmitMode,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MessageBodiesRequest {
    #[ts(type = "number[]")]
    pub message_ids: Vec<MessageId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MessageBodiesResponse {
    pub messages: Vec<MessageBody>,
}
