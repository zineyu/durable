//! 存储层的对外快照结构，以及 SQLite 子模块入口。
//! 写命令包装在 `store`。

pub(crate) mod sqlite;

use crate::message::Message;
use crate::queue::QueueItem;
use crate::run::RunRecord;
use crate::session::{SessionMetadata, SessionRecord};
use crate::workspace::WorkspaceMetadata;

/// bootstrap 所需的元数据快照。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub workspaces: Vec<WorkspaceMetadata>,
    pub sessions: Vec<SessionMetadata>,
    pub max_event_id: i64,
}

/// 单个 session 的持久快照：记录、消息树、队列与队列长度。
#[derive(Debug, Clone)]
pub(crate) struct SessionSnapshot {
    pub session: SessionRecord,
    pub busy: bool,
    pub queue_length: u32,
    pub messages: Vec<Message>,
    pub queue: Vec<QueueItem>,
    pub runs: Vec<RunRecord>,
    /// 快照读取事务开始时的全局最大事实事件 id。
    pub max_event_id: i64,
}
