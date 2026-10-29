//! 领域事实事件与事件信封：可回放、带游标的持久化历史。
//! 载荷只带事实身份与本体，session 视图在信封上；只走实时、不入库的增量在 `stream`。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::host::{HostMetadata, HostName, HostStatus};
use crate::id::{MessageId, QueueItemId, RunId, SessionId, WorkspacePath};
use crate::message::MessageSkeleton;
use crate::queue::QueueItem;
use crate::run::RunRecord;
use crate::session::SessionMetadata;
use crate::workspace::WorkspaceMetadata;

/// 持久化到 SQLite 的领域事实事件。
///
/// 载荷只带事实身份与事实本体；事实提交后的 session 视图在事件信封上（见
/// [`CommittedEvent`]），不在载荷里。同一变体因此不因元数据变化而改变。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum FactEvent {
    HostRegistered {
        host: HostMetadata,
    },
    HostRemoved {
        host: HostName,
    },
    HostUpdated {
        host: HostMetadata,
    },
    /// host 的运行状态变化。状态本身是进程状态，只有事实落库。
    HostStatusChanged {
        host: HostName,
        status: HostStatus,
        detail: Option<String>,
    },
    WorkspaceRegistered {
        workspace: WorkspaceMetadata,
    },
    WorkspaceRemoved {
        host: HostName,
        path: WorkspacePath,
    },
    SessionCreated {
        session_id: SessionId,
    },
    SessionRenamed {
        session_id: SessionId,
    },
    SessionDeleted {
        session_id: SessionId,
    },
    MessageAppended {
        session_id: SessionId,
        message: MessageSkeleton,
    },
    MessageUpdated {
        session_id: SessionId,
        message: MessageSkeleton,
    },
    ActiveLeafChanged {
        session_id: SessionId,
        #[ts(type = "number | null")]
        active_leaf: Option<MessageId>,
    },
    MessageQueued {
        session_id: SessionId,
        item: QueueItem,
    },
    QueueItemRemoved {
        session_id: SessionId,
        #[ts(type = "number")]
        item_id: QueueItemId,
    },
    SteerApplied {
        session_id: SessionId,
        #[ts(type = "number")]
        run_id: RunId,
        #[ts(type = "number")]
        item_id: QueueItemId,
        #[ts(type = "number")]
        message_id: MessageId,
    },
    SteerDowngraded {
        session_id: SessionId,
        item: QueueItem,
    },
    RunStarted {
        session_id: SessionId,
        // 装箱：run 记录新增了流式进度字段，内联会放大整个广播枚举。
        run: Box<RunRecord>,
    },
    RunFinished {
        session_id: SessionId,
        // 装箱：同 `RunStarted`。
        run: Box<RunRecord>,
    },
}

impl FactEvent {
    /// 事件所属的 session；host 与 workspace 事件不属于任何 session。
    #[must_use]
    pub fn session_id(&self) -> Option<&SessionId> {
        match self {
            Self::HostRegistered { .. }
            | Self::HostRemoved { .. }
            | Self::HostUpdated { .. }
            | Self::HostStatusChanged { .. }
            | Self::WorkspaceRegistered { .. }
            | Self::WorkspaceRemoved { .. } => None,
            Self::SessionCreated { session_id }
            | Self::SessionRenamed { session_id }
            | Self::SessionDeleted { session_id }
            | Self::MessageAppended { session_id, .. }
            | Self::MessageUpdated { session_id, .. }
            | Self::ActiveLeafChanged { session_id, .. }
            | Self::MessageQueued { session_id, .. }
            | Self::QueueItemRemoved { session_id, .. }
            | Self::SteerApplied { session_id, .. }
            | Self::SteerDowngraded { session_id, .. }
            | Self::RunStarted { session_id, .. }
            | Self::RunFinished { session_id, .. } => Some(session_id),
        }
    }
}

/// 已提交的事实事件，携带存储分配的全局事件 id 与事实提交后的 session 视图。
///
/// `session` 为空只有两种含义：该事实不属于任何 session（host 与 workspace 事件），
/// 或该事实删除了 session（`session_deleted`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommittedEvent {
    pub id: i64,
    pub event: FactEvent,
    pub session: Option<SessionMetadata>,
    pub created_at: i64,
}
