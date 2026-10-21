use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::id::{MessageId, SessionId, SessionTitle, SessionTitleError, WorkspacePath};
use crate::message::{MessageNode, MessageText};
use crate::queue::QueueItem;
use crate::run::RunRecord;

/// 自动标题取首行前多少个字符。
const AUTO_TITLE_LIMIT: usize = 50;

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
    pub(crate) id: SessionId,
    pub(crate) workspace: WorkspacePath,
    pub(crate) title: SessionTitle,
    pub(crate) title_auto: bool,
    pub(crate) created_at: i64,
    pub(crate) updated_at: i64,
    pub(crate) active_leaf: Option<MessageId>,
}

impl SessionRecord {
    #[must_use]
    pub fn metadata(&self, queue_length: u32, busy: bool) -> SessionMetadata {
        SessionMetadata {
            id: self.id.clone(),
            workspace: self.workspace.clone(),
            title: self.title.clone(),
            busy,
            queue_length,
            updated_at: self.updated_at,
        }
    }
}

/// 由用户消息推导自动标题：取首行前 50 个字符；首行为空白时取首个非空白行。
///
/// # Errors
///
/// - `SessionTitleError`：推导出的文本去掉首尾空白后为空。
pub(crate) fn derive_title(text: &MessageText) -> Result<SessionTitle, SessionTitleError> {
    let line = text
        .as_str()
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or(text.as_str());
    let truncated: String = line.chars().take(AUTO_TITLE_LIMIT).collect();
    SessionTitle::parse(&truncated)
}

/// 单个 session 的消息树快照。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SessionTree {
    pub session: SessionMetadata,
    /// 快照读取时刻的全局最大事实事件 id；快照包含该 id 及之前的全部事实。
    #[ts(type = "number")]
    pub max_event_id: i64,
    #[ts(type = "number | null")]
    pub active_leaf: Option<MessageId>,
    pub nodes: Vec<MessageNode>,
    pub queue: Vec<QueueItem>,
    pub runs: Vec<RunRecord>,
}
