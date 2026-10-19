use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::id::{QueueItemId, SessionId, WorkspacePath};
use crate::message::MessageNode;
use crate::queue::QueueItem;
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
    MessageQueued {
        session: SessionMetadata,
        item: QueueItem,
    },
    QueueItemRemoved {
        session_id: SessionId,
        #[ts(type = "number")]
        item_id: QueueItemId,
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
            | Self::MessageQueued { session, .. } => Some(&session.id),
            Self::SessionDeleted { session_id } | Self::QueueItemRemoved { session_id, .. } => {
                Some(session_id)
            }
        }
    }
}
