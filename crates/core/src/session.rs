//! session 元数据、持久记录与消息树快照；自动标题由首条用户消息推导。
//! 写入命令在 `store/session`。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::host::HostName;
use crate::id::{MessageId, SessionId, SessionTitle, SessionTitleError, WorkspacePath};
use crate::message::{MessageSkeleton, MessageText};
use crate::queue::QueueItem;
use crate::run::RunRecord;

/// 自动标题取首行前多少个字符。
const AUTO_TITLE_LIMIT: usize = 50;

/// 对外暴露的 session 元数据。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SessionMetadata {
    pub id: SessionId,
    /// session 所属 workspace 的 host。工具调用固定走它的 link。
    pub host: HostName,
    pub workspace: WorkspacePath,
    pub title: SessionTitle,
    pub busy: bool,
    pub queue_length: u32,
    #[ts(type = "number")]
    pub updated_at: i64,
}

/// session 视图里由提交处的变更清单推导的那部分。
///
/// 队列长度是队列项行数，`busy` 是存在 `running` 的 run。两者的定义只在
/// `Changeset::session_view`；存储只按行读回同一个结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionView {
    pub queue_length: u32,
    pub busy: bool,
}

impl SessionView {
    /// 空基线：没有队列项，也没有 run。新建 session 的视图由它推导。
    #[must_use]
    pub fn empty() -> Self {
        SessionView {
            queue_length: 0,
            busy: false,
        }
    }
}

/// session 的持久记录。`active_leaf` 是当前 transcript 的叶子消息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRecord {
    pub(crate) id: SessionId,
    pub(crate) host: HostName,
    pub(crate) workspace: WorkspacePath,
    pub(crate) title: SessionTitle,
    pub(crate) title_auto: bool,
    pub(crate) created_at: i64,
    pub(crate) updated_at: i64,
    pub(crate) active_leaf: Option<MessageId>,
}

impl SessionRecord {
    #[must_use]
    pub fn host(&self) -> &HostName {
        &self.host
    }

    #[must_use]
    pub fn workspace(&self) -> &WorkspacePath {
        &self.workspace
    }

    #[must_use]
    pub fn metadata(&self, view: SessionView) -> SessionMetadata {
        SessionMetadata {
            id: self.id.clone(),
            host: self.host.clone(),
            workspace: self.workspace.clone(),
            title: self.title.clone(),
            busy: view.busy,
            queue_length: view.queue_length,
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
pub struct SessionTreeSnapshot {
    pub session: SessionMetadata,
    /// 快照读取时刻的全局最大事实事件 id；快照包含该 id 及之前的全部事实。
    #[ts(type = "number")]
    pub max_event_id: i64,
    #[ts(type = "number | null")]
    pub active_leaf: Option<MessageId>,
    pub nodes: Vec<MessageSkeleton>,
    pub queue: Vec<QueueItem>,
    pub runs: Vec<RunRecord>,
}
