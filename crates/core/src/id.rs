use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

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

pub(crate) use deserialize_validated;

/// 为校验型数值 newtype 生成 `Deserialize`：先读整数，再走 `parse` 校验。
macro_rules! deserialize_validated_number {
    ($($type:ty),+ $(,)?) => {
        $(
            impl<'de> Deserialize<'de> for $type {
                fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
                where
                    D: serde::Deserializer<'de>,
                {
                    let raw = i64::deserialize(deserializer)?;
                    Self::parse(raw).map_err(serde::de::Error::custom)
                }
            }
        )+
    };
}

/// workspace 身份：规范化后的绝对路径。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, TS)]
pub struct WorkspacePath(String);

impl WorkspacePath {
    /// 解析语法合法的绝对路径。不访问文件系统。
    ///
    /// # Errors
    ///
    /// - `WorkspacePathError::NotAbsolute`：路径为空或不是绝对路径。
    pub fn parse(raw: &str) -> Result<Self, WorkspacePathError> {
        if raw.is_empty() || !Path::new(raw).is_absolute() {
            return Err(WorkspacePathError::NotAbsolute(raw.to_owned()));
        }
        Ok(Self(raw.to_owned()))
    }

    /// 解析路径并规范化为身份：路径必须存在且是目录，符号链接被解析到目标。
    ///
    /// # Errors
    ///
    /// - `WorkspacePathError::NotAbsolute`：路径为空或不是绝对路径。
    /// - `WorkspacePathError::NotFound`：规范化时路径不存在。
    /// - `WorkspacePathError::Unreadable`：规范化时无法读取路径。
    /// - `WorkspacePathError::NotDirectory`：规范化后的路径不是目录。
    /// - `WorkspacePathError::NotUtf8`：规范化后的路径不是 UTF-8。
    ///
    /// 副作用：访问文件系统。
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

    #[must_use]
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
    /// # Errors
    ///
    /// - `SessionIdError`：id 去掉首尾空白后为空。
    pub fn parse(raw: &str) -> Result<Self, SessionIdError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(SessionIdError(raw.to_owned()));
        }
        Ok(Self(trimmed.to_owned()))
    }

    /// 生成新的 session id（UUID v7，按时间有序）。不访问存储。
    #[must_use]
    pub fn generate() -> Self {
        Self(Uuid::now_v7().to_string())
    }

    #[must_use]
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
    /// # Errors
    ///
    /// - `SessionTitleError`：标题去掉首尾空白后为空。
    pub fn parse(raw: &str) -> Result<Self, SessionTitleError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(SessionTitleError);
        }
        Ok(Self(trimmed.to_owned()))
    }

    #[must_use]
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

/// 消息身份。由存储的全局分配器产生，单调递增，同一次分配永不重复。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct MessageId(i64);

impl MessageId {
    /// 从存储值构造。
    ///
    /// # Errors
    ///
    /// - `MessageIdError`：id 不是正数。
    pub fn parse(raw: i64) -> Result<Self, MessageIdError> {
        if raw <= 0 {
            return Err(MessageIdError(raw));
        }
        Ok(Self(raw))
    }

    #[must_use]
    pub fn get(self) -> i64 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageIdError(i64);

impl fmt::Display for MessageIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "消息 id 必须是正数: {}", self.0)
    }
}

impl std::error::Error for MessageIdError {}

/// 队列项身份。由存储的全局分配器产生，单调递增，同一次分配永不重复。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct QueueItemId(i64);

impl QueueItemId {
    /// 从存储值构造。
    ///
    /// # Errors
    ///
    /// - `QueueItemIdError`：id 不是正数。
    pub fn parse(raw: i64) -> Result<Self, QueueItemIdError> {
        if raw <= 0 {
            return Err(QueueItemIdError(raw));
        }
        Ok(Self(raw))
    }

    #[must_use]
    pub fn get(self) -> i64 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueItemIdError(i64);

impl fmt::Display for QueueItemIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "队列项 id 必须是正数: {}", self.0)
    }
}

impl std::error::Error for QueueItemIdError {}

/// run 身份。由存储的全局分配器产生，单调递增，同一次分配永不重复。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct RunId(i64);

impl RunId {
    /// 从存储值构造。
    ///
    /// # Errors
    ///
    /// - `RunIdError`：id 不是正数。
    pub fn parse(raw: i64) -> Result<Self, RunIdError> {
        if raw <= 0 {
            return Err(RunIdError(raw));
        }
        Ok(Self(raw))
    }

    #[must_use]
    pub fn get(self) -> i64 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunIdError(i64);

impl fmt::Display for RunIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "run id 必须是正数: {}", self.0)
    }
}

impl std::error::Error for RunIdError {}

deserialize_validated!(WorkspacePath, SessionId, SessionTitle);
deserialize_validated_number!(MessageId, QueueItemId, RunId);

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rejects_invalid_deserialized_values() {
        assert!(serde_json::from_value::<MessageId>(json!(0)).is_err());
        assert!(serde_json::from_value::<MessageId>(json!(-3)).is_err());
        assert!(serde_json::from_value::<QueueItemId>(json!(0)).is_err());
        assert!(serde_json::from_value::<QueueItemId>(json!(-1)).is_err());
        assert!(serde_json::from_value::<RunId>(json!(0)).is_err());
        assert_eq!(
            serde_json::from_value::<MessageId>(json!(7)).expect("正数是合法消息 id"),
            MessageId::parse(7).expect("正数是合法消息 id")
        );
        assert!(serde_json::from_value::<WorkspacePath>(json!("relative/path")).is_err());
        assert!(serde_json::from_value::<SessionId>(json!("   ")).is_err());
        assert!(serde_json::from_value::<SessionTitle>(json!("  ")).is_err());
    }
}
