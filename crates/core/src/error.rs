//! 内核错误：写命令的失败原因，按调用方该怎么处理分类。
//! 对外 HTTP 错误信封在 `crates/agent/src/error` 与 `crates/hub/src/error`。

use std::path::PathBuf;

use thiserror::Error;

use crate::id::{MessageId, QueueItemId, SessionId, WorkspacePath};
use crate::message::MessageRole;
use crate::node::NodeName;

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
    #[error("节点名已注册: {}", .0.as_str())]
    NodeNameTaken(NodeName),
    #[error("节点未注册: {}", .0.as_str())]
    NodeNotFound(NodeName),
}
