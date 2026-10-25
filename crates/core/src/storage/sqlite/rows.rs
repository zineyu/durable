//! 行解码：把 SQLite 行还原成领域类型，值不合约束时报 `CorruptedData`。
//! SQL 在父模块与 `apply`，本模块不含语句。

use sqlx::Row;
use sqlx::sqlite::SqliteRow;

use super::corrupted;
use crate::error::StoreError;
use crate::id::{MessageId, QueueItemId, RunId, SessionId, SessionTitle, WorkspacePath};
use crate::message::{
    ContentKind, ContentPart, MessageNode, MessageRole, MessageSkeleton, PartSkeleton, PartState,
};
use crate::node::{NodeMetadata, NodeName};
use crate::queue::QueueItem;
use crate::run::{RunFailure, RunRecord, RunStatus};
use crate::session::SessionRecord;
use crate::workspace::WorkspaceMetadata;

pub(super) fn parse_node(row: &SqliteRow) -> Result<NodeMetadata, StoreError> {
    Ok(NodeMetadata {
        name: NodeName::parse(&row.get::<String, _>("name"))
            .map_err(|error| corrupted("node.name", error))?,
        url: row.get("url"),
        enabled: row.get("enabled"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    })
}

pub(super) fn parse_workspace(row: &SqliteRow) -> Result<WorkspaceMetadata, StoreError> {
    Ok(WorkspaceMetadata {
        path: WorkspacePath::parse(&row.get::<String, _>("path"))
            .map_err(|error| corrupted("workspace.path", error))?,
        registered_at: row.get("registered_at"),
    })
}

pub(super) fn parse_session(row: &SqliteRow) -> Result<SessionRecord, StoreError> {
    let active_leaf: Option<i64> = row.get("active_leaf");
    let title_auto = match row.get::<i64, _>("title_auto") {
        0 => false,
        1 => true,
        other => return Err(corrupted("session.title_auto", other)),
    };
    Ok(SessionRecord {
        id: SessionId::parse(&row.get::<String, _>("id"))
            .map_err(|error| corrupted("session.id", error))?,
        workspace: WorkspacePath::parse(&row.get::<String, _>("workspace"))
            .map_err(|error| corrupted("session.workspace", error))?,
        title: SessionTitle::parse(&row.get::<String, _>("title"))
            .map_err(|error| corrupted("session.title", error))?,
        title_auto,
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        active_leaf: active_leaf
            .map(MessageId::parse)
            .transpose()
            .map_err(|error| corrupted("session.active_leaf", error))?,
    })
}

pub(super) fn parse_session_metadata(
    row: &SqliteRow,
) -> Result<crate::session::SessionMetadata, StoreError> {
    let session = parse_session(row)?;
    let queue_length = parse_queue_length(row)?;
    let busy = parse_busy(row);
    Ok(session.metadata(queue_length, busy))
}

pub(super) fn parse_queue_length(row: &SqliteRow) -> Result<u32, StoreError> {
    let count: i64 = row.get("queue_length");
    u32::try_from(count).map_err(|_| corrupted("session.queue_length", count))
}

pub(super) fn parse_busy(row: &SqliteRow) -> bool {
    row.get::<i64, _>("running_count") > 0
}

pub(super) fn parse_queue_item(row: &SqliteRow) -> Result<QueueItem, StoreError> {
    let anchor: Option<i64> = row.get("anchor");
    let pending_steer = match row.get::<i64, _>("pending_steer") {
        0 => false,
        1 => true,
        other => return Err(corrupted("queue_item.pending_steer", other)),
    };
    Ok(QueueItem {
        id: QueueItemId::parse(row.get::<i64, _>("id"))
            .map_err(|error| corrupted("queue_item.id", error))?,
        text: crate::message::MessageText::parse(&row.get::<String, _>("text"))
            .map_err(|error| corrupted("queue_item.text", error))?,
        anchor: anchor
            .map(MessageId::parse)
            .transpose()
            .map_err(|error| corrupted("queue_item.anchor", error))?,
        created_at: row.get("created_at"),
        pending_steer,
    })
}

/// 骨架行：消息列 + 一个块的 `seq`、`kind`、`state`、`size`。
pub(super) fn parse_skeleton(row: &SqliteRow) -> Result<MessageSkeleton, StoreError> {
    let parent_id: Option<i64> = row.get("parent_id");
    let run_id: Option<i64> = row.get("run_id");
    Ok(MessageSkeleton {
        id: MessageId::parse(row.get::<i64, _>("id"))
            .map_err(|error| corrupted("message.id", error))?,
        parent_id: parent_id
            .map(MessageId::parse)
            .transpose()
            .map_err(|error| corrupted("message.parent_id", error))?,
        role: MessageRole::parse(&row.get::<String, _>("role"))
            .map_err(|error| corrupted("message.role", error))?,
        run_id: run_id
            .map(RunId::parse)
            .transpose()
            .map_err(|error| corrupted("message.run_id", error))?,
        created_at: row.get("created_at"),
        parts: Vec::new(),
    })
}

/// 块骨架行。`size` 直接取存储中 `body` 的字节数，不读正文。
pub(super) fn parse_part_skeleton(row: &SqliteRow) -> Result<PartSkeleton, StoreError> {
    let size: i64 = row.get("size");
    Ok(PartSkeleton {
        seq: u32::try_from(row.get::<i64, _>("seq"))
            .map_err(|error| corrupted("message_part.seq", error))?,
        kind: ContentKind::parse(&row.get::<String, _>("kind"))
            .map_err(|error| corrupted("message_part.kind", error))?,
        size: u32::try_from(size).unwrap_or(u32::MAX),
        state: PartState::parse(&row.get::<String, _>("state"))
            .map_err(|error| corrupted("message_part.state", error))?,
    })
}

pub(super) fn parse_message(
    row: &SqliteRow,
    parts: Vec<ContentPart>,
) -> Result<MessageNode, StoreError> {
    let parent_id: Option<i64> = row.get("parent_id");
    let run_id: Option<i64> = row.get("run_id");
    Ok(MessageNode {
        id: MessageId::parse(row.get::<i64, _>("id"))
            .map_err(|error| corrupted("message.id", error))?,
        parent_id: parent_id
            .map(MessageId::parse)
            .transpose()
            .map_err(|error| corrupted("message.parent_id", error))?,
        role: MessageRole::parse(&row.get::<String, _>("role"))
            .map_err(|error| corrupted("message.role", error))?,
        parts,
        run_id: run_id
            .map(RunId::parse)
            .transpose()
            .map_err(|error| corrupted("message.run_id", error))?,
        created_at: row.get("created_at"),
    })
}

pub(super) fn parse_run(row: &SqliteRow) -> Result<RunRecord, StoreError> {
    Ok(RunRecord {
        id: RunId::parse(row.get::<i64, _>("id")).map_err(|error| corrupted("run.id", error))?,
        session_id: SessionId::parse(&row.get::<String, _>("session_id"))
            .map_err(|error| corrupted("run.session_id", error))?,
        status: RunStatus::parse(&row.get::<String, _>("status"))
            .map_err(|error| corrupted("run.status", error))?,
        failure: row
            .get::<Option<String>, _>("failure")
            .map(|raw| {
                serde_json::from_str::<RunFailure>(&raw)
                    .map_err(|error| corrupted("run.failure", error))
            })
            .transpose()?,
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    })
}
