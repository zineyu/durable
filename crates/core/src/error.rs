use thiserror::Error;

use crate::id::{SessionId, WorkspacePath};

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("SQLite 错误: {0}")]
    Database(#[from] sqlx::Error),
    #[error("迁移错误: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),
    #[error("事件序列化错误: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("数据库中的值不符合领域约束: {0}")]
    CorruptedData(String),
    #[error("workspace 已注册: {}", .0.as_str())]
    WorkspaceDuplicate(WorkspacePath),
    #[error("workspace 未注册: {}", .0.as_str())]
    WorkspaceNotFound(WorkspacePath),
    #[error("workspace 仍有 session: {}", .0.as_str())]
    WorkspaceNotEmpty(WorkspacePath),
    #[error("session 不存在: {}", .0.as_str())]
    SessionNotFound(SessionId),
}
