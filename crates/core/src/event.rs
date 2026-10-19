use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::id::{MessageId, QueueItemId, SessionId, WorkspacePath};
use crate::message::MessageNode;
use crate::queue::QueueItem;
use crate::run::RunRecord;
use crate::session::SessionMetadata;
use crate::workspace::WorkspaceMetadata;

/// 持久化到 SQLite 的领域事实事件。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum FactEvent {
    WorkspaceRegistered {
        workspace: WorkspaceMetadata,
    },
    WorkspaceRemoved {
        path: WorkspacePath,
    },
    SessionCreated {
        session: SessionMetadata,
    },
    SessionRenamed {
        session: SessionMetadata,
    },
    SessionDeleted {
        session_id: SessionId,
    },
    MessageAppended {
        session: SessionMetadata,
        message: MessageNode,
    },
    MessageUpdated {
        session: SessionMetadata,
        message: MessageNode,
    },
    ActiveLeafChanged {
        session: SessionMetadata,
        #[ts(type = "number | null")]
        active_leaf: Option<MessageId>,
    },
    MessageQueued {
        session: SessionMetadata,
        item: QueueItem,
    },
    QueueItemRemoved {
        session_id: SessionId,
        #[ts(type = "number")]
        item_id: QueueItemId,
    },
    RunStarted {
        session: SessionMetadata,
        run: RunRecord,
    },
    RunFinished {
        session: SessionMetadata,
        run: RunRecord,
    },
}

impl FactEvent {
    /// 事件所属的 session；workspace 事件不属于任何 session。
    pub fn session_id(&self) -> Option<&SessionId> {
        match self {
            Self::WorkspaceRegistered { .. } | Self::WorkspaceRemoved { .. } => None,
            Self::SessionCreated { session }
            | Self::SessionRenamed { session }
            | Self::MessageAppended { session, .. }
            | Self::MessageUpdated { session, .. }
            | Self::ActiveLeafChanged { session, .. }
            | Self::MessageQueued { session, .. }
            | Self::RunStarted { session, .. }
            | Self::RunFinished { session, .. } => Some(&session.id),
            Self::SessionDeleted { session_id } | Self::QueueItemRemoved { session_id, .. } => {
                Some(session_id)
            }
        }
    }
}

/// 已提交的事实事件，携带存储分配的全局事件 id。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommittedEvent {
    pub id: i64,
    pub event: FactEvent,
    pub created_at: i64,
}
