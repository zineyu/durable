use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

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

/// 为校验型 newtype 生成 `Deserialize`：先读字符串，再走 `parse` 校验。
macro_rules! deserialize_validated {
    ($($type:ty),+ $(,)?) => {
        $(
            impl<'de> Deserialize<'de> for $type {
                fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
                where
                    D: serde::Deserializer<'de>,
                {
                    let raw = String::deserialize(deserializer)?;
                    Self::parse(&raw).map_err(serde::de::Error::custom)
                }
            }
        )+
    };
}

deserialize_validated!(WorkspacePath, SessionId, SessionTitle, MessageId);

/// workspace 身份：规范化后的绝对路径。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, TS)]
pub struct WorkspacePath(String);

impl WorkspacePath {
    /// 解析语法合法的绝对路径。不访问文件系统。
    ///
    /// 失败：路径为空或不是绝对路径。
    pub fn parse(raw: &str) -> Result<Self, WorkspacePathError> {
        if raw.is_empty() || !Path::new(raw).is_absolute() {
            return Err(WorkspacePathError::NotAbsolute(raw.to_owned()));
        }
        Ok(Self(raw.to_owned()))
    }

    /// 解析路径并规范化为身份：路径必须存在且是目录，符号链接被解析到目标。
    ///
    /// 失败：路径不是绝对路径、不存在、不是目录、不是 UTF-8，或无法读取。
    pub fn resolve(raw: &str) -> Result<Self, WorkspacePathError> {
        let path = Self::parse(raw)?;
        let canonical = std::fs::canonicalize(&path.0).map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => WorkspacePathError::NotFound(raw.to_owned()),
            _ => WorkspacePathError::Unreadable(raw.to_owned(), error.to_string()),
        })?;
        if !canonical.is_dir() {
            return Err(WorkspacePathError::NotDirectory(raw.to_owned()));
        }
        let canonical = canonical
            .to_str()
            .ok_or_else(|| WorkspacePathError::NotUtf8(raw.to_owned()))?;
        Ok(Self(canonical.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspacePathError {
    NotAbsolute(String),
    NotUtf8(String),
    NotFound(String),
    NotDirectory(String),
    Unreadable(String, String),
}

impl fmt::Display for WorkspacePathError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAbsolute(raw) => write!(formatter, "workspace 路径必须是绝对路径: {raw}"),
            Self::NotUtf8(raw) => write!(formatter, "workspace 路径不是 UTF-8: {raw}"),
            Self::NotFound(raw) => write!(formatter, "workspace 路径不存在: {raw}"),
            Self::NotDirectory(raw) => write!(formatter, "workspace 路径不是目录: {raw}"),
            Self::Unreadable(raw, detail) => {
                write!(formatter, "workspace 路径无法读取: {raw} ({detail})")
            }
        }
    }
}

impl std::error::Error for WorkspacePathError {}

/// session 身份。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, TS)]
pub struct SessionId(String);

impl SessionId {
    /// 解析 session id。
    ///
    /// 失败：id 为空。
    pub fn parse(raw: &str) -> Result<Self, SessionIdError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(SessionIdError(raw.to_owned()));
        }
        Ok(Self(trimmed.to_owned()))
    }

    /// 生成新的 session id（UUID v7，按时间有序）。
    pub fn generate() -> Self {
        Self(Uuid::now_v7().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionIdError(String);

impl fmt::Display for SessionIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "session id 不能为空: {}", self.0)
    }
}

impl std::error::Error for SessionIdError {}

/// session 标题。构造时去掉首尾空白，因此总是非空。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct SessionTitle(String);

pub const DEFAULT_SESSION_TITLE: &str = "新 session";

impl SessionTitle {
    /// 解析标题并去掉首尾空白。
    ///
    /// 失败：去掉空白后为空。
    pub fn parse(raw: &str) -> Result<Self, SessionTitleError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(SessionTitleError);
        }
        Ok(Self(trimmed.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for SessionTitle {
    fn default() -> Self {
        Self(DEFAULT_SESSION_TITLE.to_owned())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionTitleError;

impl fmt::Display for SessionTitleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "session 标题不能为空")
    }
}

impl std::error::Error for SessionTitleError {}

// i64 字段是毫秒时间戳与事件 id，均小于 JS 安全整数上限；TS 侧映射为 number。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceMetadata {
    pub path: WorkspacePath,
    #[ts(type = "number")]
    pub registered_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SessionMetadata {
    pub id: SessionId,
    pub workspace: WorkspacePath,
    pub title: SessionTitle,
    pub busy: bool,
    pub queue_length: u32,
    #[ts(type = "number")]
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapResponse {
    pub model: String,
    #[ts(type = "number")]
    pub max_event_id: i64,
    pub workspaces: Vec<WorkspaceMetadata>,
    pub sessions: Vec<SessionMetadata>,
}

/// 消息节点身份。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, TS)]
pub struct MessageId(String);

impl MessageId {
    /// 解析消息节点 id。
    ///
    /// 失败：id 为空。
    pub fn parse(raw: &str) -> Result<Self, MessageIdError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(MessageIdError(raw.to_owned()));
        }
        Ok(Self(trimmed.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageIdError(String);

impl fmt::Display for MessageIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "消息 id 不能为空: {}", self.0)
    }
}

impl std::error::Error for MessageIdError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MessageNode {
    pub id: MessageId,
    pub parent_id: Option<MessageId>,
    pub role: MessageRole,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SessionTree {
    pub session: SessionMetadata,
    pub active_leaf: Option<MessageId>,
    pub nodes: Vec<MessageNode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum FactEvent {
    WorkspaceRegistered { workspace: WorkspaceMetadata },
    WorkspaceRemoved { path: WorkspacePath },
    SessionCreated { session: SessionMetadata },
    SessionRenamed { session: SessionMetadata },
    SessionDeleted { session_id: SessionId },
}

impl FactEvent {
    /// 事件所属的 session；workspace 事件不属于任何 session。
    pub fn session_id(&self) -> Option<&SessionId> {
        match self {
            Self::WorkspaceRegistered { .. } | Self::WorkspaceRemoved { .. } => None,
            Self::SessionCreated { session } | Self::SessionRenamed { session } => {
                Some(&session.id)
            }
            Self::SessionDeleted { session_id } => Some(session_id),
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
    InvalidTitle,
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
