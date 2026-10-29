//! agent 的 HTTP 协议 DTO 与协议版本。
//! hub 自有协议在 `crates/hub/src/protocol`。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use durable::{
    HostKind, HostMetadata, HostName, HostStatus, MessageBody, MessageId, SessionMetadata,
    SubmitMode, WorkspaceMetadata,
};

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
/// 7：host 成为领域对象。bootstrap 多出 `hosts`，workspace 与 session 带上 `host`，
///    workspace 注册请求与 session 创建请求都要写 host，`errorCode` 用 host 码取代
///    `node_name_taken` 与 `node_not_found`。旧客户端把 workspace 的 host 读丢了，
///    因此不兼容。
pub const PROTOCOL_VERSION: u32 = 7;

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
    /// host 表与每个 host 的运行状态。状态是进程状态，不落库。
    pub hosts: Vec<HostSnapshot>,
    pub workspaces: Vec<WorkspaceMetadata>,
    pub sessions: Vec<SessionMetadata>,
}

/// 侧栏看到的单个 host：落库的元数据加上进程内的运行状态。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct HostSnapshot {
    pub name: HostName,
    pub kind: HostKind,
    pub target: Option<String>,
    /// 停用的 host 不再重连，命令一律返回不可用。
    pub enabled: bool,
    pub status: HostStatus,
    /// 状态的具体说明；`ok` 时为 `None`。
    pub detail: Option<String>,
    /// 最近一次心跳的往返毫秒数。还没收到过应答时为 `None`。不落库。
    #[ts(type = "number | null")]
    pub latency_ms: Option<u64>,
}

impl HostSnapshot {
    /// 由落库的元数据与进程内的运行状态组装。
    #[must_use]
    pub fn of(
        host: HostMetadata,
        status: HostStatus,
        detail: Option<String>,
        latency_ms: Option<u64>,
    ) -> Self {
        Self {
            name: host.name,
            kind: host.kind,
            target: host.target,
            enabled: host.enabled,
            status,
            detail,
            latency_ms,
        }
    }
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
    HostNotFound,
    HostNameTaken,
    HostNameInvalid,
    HostInvalidTarget,
    HostUnavailable,
    HostInUse,
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
    /// workspace 所属的 host。路径校验在这台 host 上完成。
    pub host: HostName,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RemoveWorkspaceRequest {
    /// workspace 身份是 host 与路径这一对。
    pub host: HostName,
    pub path: String,
}

/// 注册 host。`kind` 为 `ssh` 时 `target` 必填。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RegisterHostRequest {
    pub name: String,
    pub kind: HostKind,
    pub target: Option<String>,
}

/// 局部更新 host。`None` 表示保留原值；`name` 是改名后的 host 名。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct UpdateHostRequest {
    pub name: Option<String>,
    pub target: Option<String>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct CreateSessionRequest {
    /// session 继承 workspace 的 host：创建时必须写明是哪台 host 上的哪个目录。
    pub host: HostName,
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
