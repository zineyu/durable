//! 存储层的快照结构、恢复后不变量报告与 SQLite 子模块入口：
//! 命令入口用的轻量快照、读取用的完整快照，以及 `check_invariants` 的违例清单。
//! 写命令入口在 `store/command`。

pub(crate) mod sqlite;

use std::fmt;

use crate::host::HostMetadata;
use crate::message::MessageSkeleton;
use crate::queue::QueueItem;
use crate::run::RunRecord;
use crate::session::{SessionMetadata, SessionRecord, SessionView};
use crate::workspace::WorkspaceMetadata;

/// bootstrap 所需的元数据快照。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub hosts: Vec<HostMetadata>,
    pub workspaces: Vec<WorkspaceMetadata>,
    pub sessions: Vec<SessionMetadata>,
    pub max_event_id: i64,
}

/// 单个 session 的轻量快照：记录与视图。视图里的队列长度与 `busy` 是读回值，
/// 本次写命令在此基础上推导新视图。
///
/// 命令入口只读它：全量快照会加载全部消息，写热路径不能承担。
#[derive(Debug, Clone)]
pub(crate) struct SessionHead {
    pub session: SessionRecord,
    pub view: SessionView,
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

/// 恢复后不变量的一条违例。`kind` 是稳定类别，`detail` 指出具体行。
///
/// 调用点是 [`crate::Store::check_invariants`]：`RunEngine::round_zero` 修复完、run
/// 真正开始之前检查一次，崩溃前缀矩阵在每个重启点检查一次（ADR 0056）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvariantViolation {
    pub kind: InvariantKind,
    pub detail: String,
}

impl fmt::Display for InvariantViolation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.kind.as_str(), self.detail)
    }
}

/// 不变量类别。名字进日志与测试失败信息，因此保持稳定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvariantKind {
    /// 同一 session 有多条 `running` 的 run。
    RunningRunPerSession,
    /// `progress` 留在已经结束的 run 上。
    ProgressOnFinishedRun,
    /// 没有 `running` 的 run，却有执行中的工具节点。
    RunningToolWithoutRunningRun,
    /// 同一个调用有多个工具结局节点。
    DuplicateToolResult,
    /// 工具结局找不到对应的调用。
    UnpairedToolResult,
    /// active leaf 不在消息表里。
    ActiveLeafOffTree,
    /// 队列项锚定的消息不在表里。
    QueueAnchorOffTree,
    /// id 分配器落后于表里已有的最大 id，下一次铸造会撞车。
    AllocatorBehind,
    /// 已结束的 run 里，assistant 声明过的调用缺工具结局。
    TerminalRunMissingToolResult,
}

impl InvariantKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RunningRunPerSession => "running_run_per_session",
            Self::ProgressOnFinishedRun => "progress_on_finished_run",
            Self::RunningToolWithoutRunningRun => "running_tool_without_running_run",
            Self::DuplicateToolResult => "duplicate_tool_result",
            Self::UnpairedToolResult => "unpaired_tool_result",
            Self::ActiveLeafOffTree => "active_leaf_off_tree",
            Self::QueueAnchorOffTree => "queue_anchor_off_tree",
            Self::AllocatorBehind => "allocator_behind",
            Self::TerminalRunMissingToolResult => "terminal_run_missing_tool_result",
        }
    }
}
