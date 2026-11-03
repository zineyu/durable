//! 内核错误：写命令的失败原因与它的结局类别。
//! 对外 HTTP 错误信封在 `crates/agent/src/error`。

use std::path::PathBuf;

use thiserror::Error;

use crate::host::HostName;
use crate::id::{MessageId, QueueItemId, SessionId, WorkspacePath};
use crate::message::MessageRole;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("SQLite 错误: {0}")]
    Database(#[from] sqlx::Error),
    #[error("迁移错误: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),
    #[error("无法建立数据库目录 {}: {message}", path.display())]
    DatabaseDirectory { path: PathBuf, message: String },
    #[error("事件序列化错误: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("数据库中的值不符合领域约束: {0}")]
    CorruptedData(String),
    #[error("数据库是旧结构，不提供导入；请改用新的数据库路径: {}", .0.display())]
    LegacyDatabase(PathBuf),
    #[error("写入违反内核不变量: {0}")]
    Invariant(String),
    #[error("workspace 已注册: {}", .0.as_str())]
    WorkspaceDuplicate(WorkspacePath),
    #[error("workspace 未注册: {}", .0.as_str())]
    WorkspaceNotFound(WorkspacePath),
    #[error("workspace 仍有 session: {}", .0.as_str())]
    WorkspaceNotEmpty(WorkspacePath),
    #[error("session 不存在: {}", .0.as_str())]
    SessionNotFound(SessionId),
    #[error("队列项不存在: {}", .1.get())]
    QueueItemNotFound(SessionId, QueueItemId),
    #[error("消息不存在: {}", .1.get())]
    MessageNotFound(SessionId, MessageId),
    #[error("消息角色不匹配: {} 是 {}", .0.get(), .1.as_str())]
    MessageRoleMismatch(MessageId, MessageRole),
    #[error("session 有 run 进行中: {}", .0.as_str())]
    RunInProgress(SessionId),
    #[error("session 的队列非空: {}", .0.as_str())]
    QueueNotEmpty(SessionId),
    #[error("host 名已注册: {}", .0.as_str())]
    HostNameTaken(HostName),
    #[error("host 未注册: {}", .0.as_str())]
    HostNotFound(HostName),
    #[error("host 仍有 {} 个 workspace，不能删除: {}", .1, .0.as_str())]
    HostNotEmpty(HostName, i64),
}

impl StoreError {
    /// 本错误的结局类别。状态码只由它决定，调用方不自行解释变体。
    ///
    /// 引擎不区分其中的 `Internal` 与 `StorageFailure`：任何 `StoreError` 都记为
    /// `RunFailureCode::StorageFailed`（ADR 0043、ADR 0054）。
    #[must_use]
    pub fn class(&self) -> StoreErrorClass {
        match self {
            Self::Database(_)
            | Self::Migration(_)
            | Self::DatabaseDirectory { .. }
            | Self::LegacyDatabase(_) => StoreErrorClass::StorageFailure,
            Self::Serialization(_) | Self::CorruptedData(_) | Self::Invariant(_) => {
                StoreErrorClass::Internal
            }
            Self::WorkspaceDuplicate(_)
            | Self::WorkspaceNotEmpty(_)
            | Self::RunInProgress(_)
            | Self::QueueNotEmpty(_)
            | Self::HostNameTaken(_)
            | Self::HostNotEmpty(_, _) => StoreErrorClass::Conflict,
            Self::WorkspaceNotFound(_)
            | Self::SessionNotFound(_)
            | Self::QueueItemNotFound(_, _)
            | Self::MessageNotFound(_, _)
            | Self::HostNotFound(_) => StoreErrorClass::NotFound,
            Self::MessageRoleMismatch(_, _) => StoreErrorClass::InvalidArgument,
        }
    }
}

/// 一次失败属于哪类结局。不含 HTTP 词汇，只表达结局类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreErrorClass {
    /// 目标不存在。
    NotFound,
    /// 与既有状态冲突。
    Conflict,
    /// 输入不满足领域约束。
    InvalidArgument,
    /// 存储的值或内核不变量已损坏。
    Internal,
    /// 存储本身不可用：连接、迁移或目录。
    StorageFailure,
}

#[cfg(test)]
mod tests {
    use crate::host::HostName;
    use crate::id::{MessageId, QueueItemId, SessionId, WorkspacePath};
    use crate::message::MessageRole;

    use super::{StoreError, StoreErrorClass};

    /// 每个变体的结局类别。这是独立事实源：新增变体时必须在这里表态。
    fn every_error() -> Vec<(StoreError, StoreErrorClass)> {
        let workspace = WorkspacePath::parse("/workspace").expect("合法 workspace 路径");
        let session = SessionId::parse("session").expect("合法 session id");
        let message = MessageId::parse(7).expect("合法消息 id");
        let queue_item = QueueItemId::parse(3).expect("合法队列项 id");
        let host = HostName::parse("local").expect("合法 host 名");
        vec![
            (
                StoreError::Database(sqlx::Error::RowNotFound),
                StoreErrorClass::StorageFailure,
            ),
            (
                StoreError::Migration(sqlx::migrate::MigrateError::VersionMissing(1)),
                StoreErrorClass::StorageFailure,
            ),
            (
                StoreError::DatabaseDirectory {
                    path: "/nowhere".into(),
                    message: "测试".to_owned(),
                },
                StoreErrorClass::StorageFailure,
            ),
            (
                StoreError::Serialization(
                    serde_json::from_str::<i32>("不是 JSON").expect_err("解析失败"),
                ),
                StoreErrorClass::Internal,
            ),
            (
                StoreError::CorruptedData("message_part.body".to_owned()),
                StoreErrorClass::Internal,
            ),
            (
                StoreError::LegacyDatabase("/nowhere/old.sqlite".into()),
                StoreErrorClass::StorageFailure,
            ),
            (
                StoreError::Invariant("测试".to_owned()),
                StoreErrorClass::Internal,
            ),
            (
                StoreError::WorkspaceDuplicate(workspace.clone()),
                StoreErrorClass::Conflict,
            ),
            (
                StoreError::WorkspaceNotFound(workspace.clone()),
                StoreErrorClass::NotFound,
            ),
            (
                StoreError::WorkspaceNotEmpty(workspace),
                StoreErrorClass::Conflict,
            ),
            (
                StoreError::SessionNotFound(session.clone()),
                StoreErrorClass::NotFound,
            ),
            (
                StoreError::QueueItemNotFound(session.clone(), queue_item),
                StoreErrorClass::NotFound,
            ),
            (
                StoreError::MessageNotFound(session.clone(), message),
                StoreErrorClass::NotFound,
            ),
            (
                StoreError::MessageRoleMismatch(message, MessageRole::User),
                StoreErrorClass::InvalidArgument,
            ),
            (
                StoreError::RunInProgress(session.clone()),
                StoreErrorClass::Conflict,
            ),
            (
                StoreError::QueueNotEmpty(session),
                StoreErrorClass::Conflict,
            ),
            (
                StoreError::HostNameTaken(host.clone()),
                StoreErrorClass::Conflict,
            ),
            (
                StoreError::HostNotFound(host.clone()),
                StoreErrorClass::NotFound,
            ),
            (StoreError::HostNotEmpty(host, 2), StoreErrorClass::Conflict),
        ]
    }

    /// ADR 0043 与 ADR 0054：结局类别由内核在 `StoreError` 上直接给出。
    #[test]
    fn every_error_reports_its_class() {
        for (error, class) in every_error() {
            assert_eq!(error.class(), class, "{error}");
        }
    }
}
