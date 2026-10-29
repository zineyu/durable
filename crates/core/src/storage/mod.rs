//! 存储层的快照结构与 SQLite 子模块入口：命令入口用的轻量快照与读取用的完整快照。
//! 写命令入口在 `store/command`。

pub(crate) mod sqlite;

use crate::host::HostMetadata;
use crate::message::MessageSkeleton;
use crate::queue::QueueItem;
use crate::run::RunRecord;
use crate::session::{SessionMetadata, SessionRecord};
use crate::workspace::WorkspaceMetadata;

/// bootstrap 所需的元数据快照。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub hosts: Vec<HostMetadata>,
    pub workspaces: Vec<WorkspaceMetadata>,
    pub sessions: Vec<SessionMetadata>,
    pub max_event_id: i64,
}

/// 单个 session 的轻量快照：记录、队列长度与是否有 running 的 run。
///
/// 命令入口只读它：全量快照会加载全部消息，写热路径不能承担。
#[derive(Debug, Clone)]
pub(crate) struct SessionHead {
    pub session: SessionRecord,
    pub queue_length: u32,
    pub busy: bool,
}

/// 单个 session 的完整快照：轻量快照、消息树、队列与 run 列表。
#[derive(Debug, Clone)]
pub(crate) struct SessionSnapshot {
    pub head: SessionHead,
    pub messages: Vec<MessageSkeleton>,
    pub queue: Vec<QueueItem>,
    pub runs: Vec<RunRecord>,
    /// 快照读取事务开始时的全局最大事实事件 id。
    pub max_event_id: i64,
}
