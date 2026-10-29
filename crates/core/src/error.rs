//! 内核错误：写命令的失败原因，以及它的两层分类（分类与结局类别）。
//! 对外 HTTP 错误信封在 `crates/agent/src/error` 与 `crates/hub/src/error`。

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
    /// 本错误的分类。分类不带载荷，与变体一一对应。
    ///
    /// 调用方先取分类，再取 [`StoreErrorKind::class`]，不需要再匹配带载荷的变体。
    #[must_use]
    pub fn kind(&self) -> StoreErrorKind {
        match self {
            Self::Database(_) => StoreErrorKind::Database,
            Self::Migration(_) => StoreErrorKind::Migration,
            Self::DatabaseDirectory { .. } => StoreErrorKind::DatabaseDirectory,
            Self::Serialization(_) => StoreErrorKind::Serialization,
            Self::CorruptedData(_) => StoreErrorKind::CorruptedData,
            Self::LegacyDatabase(_) => StoreErrorKind::LegacyDatabase,
            Self::Invariant(_) => StoreErrorKind::Invariant,
            Self::WorkspaceDuplicate(_) => StoreErrorKind::WorkspaceDuplicate,
            Self::WorkspaceNotFound(_) => StoreErrorKind::WorkspaceNotFound,
            Self::WorkspaceNotEmpty(_) => StoreErrorKind::WorkspaceNotEmpty,
            Self::SessionNotFound(_) => StoreErrorKind::SessionNotFound,
            Self::QueueItemNotFound(_, _) => StoreErrorKind::QueueItemNotFound,
            Self::MessageNotFound(_, _) => StoreErrorKind::MessageNotFound,
            Self::MessageRoleMismatch(_, _) => StoreErrorKind::MessageRoleMismatch,
            Self::RunInProgress(_) => StoreErrorKind::RunInProgress,
            Self::QueueNotEmpty(_) => StoreErrorKind::QueueNotEmpty,
            Self::HostNameTaken(_) => StoreErrorKind::HostNameTaken,
            Self::HostNotFound(_) => StoreErrorKind::HostNotFound,
            Self::HostNotEmpty(_, _) => StoreErrorKind::HostNotEmpty,
        }
    }
}

/// [`StoreError`] 的分类，与变体一一对应且不带载荷。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreErrorKind {
    Database,
    Migration,
    DatabaseDirectory,
    Serialization,
    CorruptedData,
    LegacyDatabase,
    Invariant,
    WorkspaceDuplicate,
    WorkspaceNotFound,
    WorkspaceNotEmpty,
    SessionNotFound,
    QueueItemNotFound,
    MessageNotFound,
    MessageRoleMismatch,
    RunInProgress,
    QueueNotEmpty,
    HostNameTaken,
    HostNotFound,
    HostNotEmpty,
}

impl StoreErrorKind {
    /// 本分类属于哪种结局。
    ///
    /// 结局类别是内核的判断：调用方据此决定状态码，不自行解释分类。
    /// 引擎不区分其中的 `Internal` 与 `StorageFailure`，任何 `StoreError` 都记为
    /// `RunFailureCode::StorageFailed`（见 ADR 0043）。
    #[must_use]
    pub fn class(self) -> StoreErrorClass {
        match self {
            Self::Database | Self::Migration | Self::DatabaseDirectory | Self::LegacyDatabase => {
                StoreErrorClass::StorageFailure
            }
            Self::Serialization | Self::CorruptedData | Self::Invariant => {
                StoreErrorClass::Internal
            }
            Self::WorkspaceDuplicate
            | Self::WorkspaceNotEmpty
            | Self::RunInProgress
            | Self::QueueNotEmpty
            | Self::HostNameTaken
            | Self::HostNotEmpty => StoreErrorClass::Conflict,
            Self::WorkspaceNotFound
            | Self::SessionNotFound
            | Self::QueueItemNotFound
            | Self::MessageNotFound
            | Self::HostNotFound => StoreErrorClass::NotFound,
            Self::MessageRoleMismatch => StoreErrorClass::InvalidArgument,
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

    use super::{StoreError, StoreErrorClass, StoreErrorKind};

    /// 分类表的独立事实源：新增变体时必须在这里给出它的结局类别。
    const TABLE: [(StoreErrorKind, StoreErrorClass); 19] = [
        (StoreErrorKind::Database, StoreErrorClass::StorageFailure),
        (StoreErrorKind::Migration, StoreErrorClass::StorageFailure),
        (
            StoreErrorKind::DatabaseDirectory,
            StoreErrorClass::StorageFailure,
        ),
        (StoreErrorKind::Serialization, StoreErrorClass::Internal),
        (StoreErrorKind::CorruptedData, StoreErrorClass::Internal),
        (
            StoreErrorKind::LegacyDatabase,
            StoreErrorClass::StorageFailure,
        ),
        (StoreErrorKind::Invariant, StoreErrorClass::Internal),
        (
            StoreErrorKind::WorkspaceDuplicate,
            StoreErrorClass::Conflict,
        ),
        (StoreErrorKind::WorkspaceNotFound, StoreErrorClass::NotFound),
        (StoreErrorKind::WorkspaceNotEmpty, StoreErrorClass::Conflict),
        (StoreErrorKind::SessionNotFound, StoreErrorClass::NotFound),
        (StoreErrorKind::QueueItemNotFound, StoreErrorClass::NotFound),
        (StoreErrorKind::MessageNotFound, StoreErrorClass::NotFound),
        (
            StoreErrorKind::MessageRoleMismatch,
            StoreErrorClass::InvalidArgument,
        ),
        (StoreErrorKind::RunInProgress, StoreErrorClass::Conflict),
        (StoreErrorKind::QueueNotEmpty, StoreErrorClass::Conflict),
        (StoreErrorKind::HostNameTaken, StoreErrorClass::Conflict),
        (StoreErrorKind::HostNotFound, StoreErrorClass::NotFound),
        (StoreErrorKind::HostNotEmpty, StoreErrorClass::Conflict),
    ];

    /// 19 个变体到分类的映射。带载荷的变体在这里给出真实载荷，
    /// 因此 `StoreError::kind` 的每个分支都被走到。
    fn every_error() -> Vec<(StoreError, StoreErrorKind)> {
        let workspace = WorkspacePath::parse("/workspace").expect("合法 workspace 路径");
        let session = SessionId::parse("session").expect("合法 session id");
        let message = MessageId::parse(7).expect("合法消息 id");
        let queue_item = QueueItemId::parse(3).expect("合法队列项 id");
        let host = HostName::parse("local").expect("合法 host 名");
        vec![
            (
                StoreError::Database(sqlx::Error::RowNotFound),
                StoreErrorKind::Database,
            ),
            (
                StoreError::Migration(sqlx::migrate::MigrateError::VersionMissing(1)),
                StoreErrorKind::Migration,
            ),
            (
                StoreError::DatabaseDirectory {
                    path: "/nowhere".into(),
                    message: "测试".to_owned(),
                },
                StoreErrorKind::DatabaseDirectory,
            ),
            (
                StoreError::Serialization(
                    serde_json::from_str::<i32>("不是 JSON").expect_err("解析失败"),
                ),
                StoreErrorKind::Serialization,
            ),
            (
                StoreError::CorruptedData("message_part.body".to_owned()),
                StoreErrorKind::CorruptedData,
            ),
            (
                StoreError::LegacyDatabase("/nowhere/old.sqlite".into()),
                StoreErrorKind::LegacyDatabase,
            ),
            (
                StoreError::Invariant("测试".to_owned()),
                StoreErrorKind::Invariant,
            ),
            (
                StoreError::WorkspaceDuplicate(workspace.clone()),
                StoreErrorKind::WorkspaceDuplicate,
            ),
            (
                StoreError::WorkspaceNotFound(workspace.clone()),
                StoreErrorKind::WorkspaceNotFound,
            ),
            (
                StoreError::WorkspaceNotEmpty(workspace),
                StoreErrorKind::WorkspaceNotEmpty,
            ),
            (
                StoreError::SessionNotFound(session.clone()),
                StoreErrorKind::SessionNotFound,
            ),
            (
                StoreError::QueueItemNotFound(session.clone(), queue_item),
                StoreErrorKind::QueueItemNotFound,
            ),
            (
                StoreError::MessageNotFound(session.clone(), message),
                StoreErrorKind::MessageNotFound,
            ),
            (
                StoreError::MessageRoleMismatch(message, MessageRole::User),
                StoreErrorKind::MessageRoleMismatch,
            ),
            (
                StoreError::RunInProgress(session.clone()),
                StoreErrorKind::RunInProgress,
            ),
            (
                StoreError::QueueNotEmpty(session),
                StoreErrorKind::QueueNotEmpty,
            ),
            (
                StoreError::HostNameTaken(host.clone()),
                StoreErrorKind::HostNameTaken,
            ),
            (
                StoreError::HostNotFound(host.clone()),
                StoreErrorKind::HostNotFound,
            ),
            (
                StoreError::HostNotEmpty(host, 2),
                StoreErrorKind::HostNotEmpty,
            ),
        ]
    }

    /// ADR 0043：第一层分类由 `StoreError` 穷尽给出。
    #[test]
    fn every_error_reports_its_kind() {
        for (error, kind) in every_error() {
            assert_eq!(error.kind(), kind, "{error}");
        }
    }

    /// ADR 0043：第二层结局类别由内核决定。
    #[test]
    fn every_kind_belongs_to_its_class() {
        for (kind, class) in TABLE {
            assert_eq!(kind.class(), class, "{kind:?}");
        }
    }
}
