use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::id::{MessageId, SessionId, SessionTitle, WorkspacePath};
use crate::message::MessageNode;

/// 对外暴露的 session 元数据。
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

/// session 的持久记录。`active_leaf` 是当前 transcript 的叶子消息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRecord {
    pub id: SessionId,
    pub workspace: WorkspacePath,
    pub title: SessionTitle,
    pub created_at: i64,
    pub updated_at: i64,
    pub active_leaf: Option<MessageId>,
}

impl SessionRecord {
    pub fn metadata(&self) -> SessionMetadata {
        SessionMetadata {
            id: self.id.clone(),
            workspace: self.workspace.clone(),
            title: self.title.clone(),
            busy: false,
            queue_length: 0,
            updated_at: self.updated_at,
        }
    }
}

/// 单个 session 的消息树快照。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SessionTree {
    pub session: SessionMetadata,
    #[ts(type = "number | null")]
    pub active_leaf: Option<MessageId>,
    pub nodes: Vec<MessageNode>,
}
