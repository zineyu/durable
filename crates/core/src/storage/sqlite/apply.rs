//! `Changeset` 落库：按变更顺序写行，并在写事件行时收集已提交事件。
//! 读路径在父模块，行解码在 `rows`。

use sqlx::sqlite::{SqliteConnection, SqliteQueryResult};

use crate::change::Change;
use crate::error::StoreError;
use crate::event::{CommittedEvent, FactEvent};
use crate::id::{MessageId, QueueItemId, RunId, SessionId, SessionTitle, WorkspacePath};
use crate::message::{ContentPart, Message};
use crate::node::{NodeMetadata, NodeName};
use crate::queue::QueueItem;
use crate::run::{RunFailure, RunRecord, RunStatus};
use crate::session::SessionRecord;
use crate::workspace::WorkspaceMetadata;

/// 把单条变更写入事务；事实事件在写入后追加到 `committed`。
pub(super) async fn apply_change(
    connection: &mut SqliteConnection,
    change: &Change,
    committed: &mut Vec<CommittedEvent>,
) -> Result<(), StoreError> {
    match change {
        Change::InsertWorkspace(workspace) => insert_workspace(connection, workspace).await,
        Change::DeleteWorkspace(path) => delete_workspace(connection, path).await,
        Change::InsertNode(node) => insert_node(connection, node).await,
        Change::UpdateNode(node) => update_node(connection, node).await,
        Change::DeleteNode(name) => delete_node(connection, name).await,
        Change::InsertSession(session) => insert_session(connection, session).await,
        Change::UpdateSession {
            id,
            title,
            title_auto,
            updated_at,
            active_leaf,
        } => {
            update_session(
                connection,
                id,
                title,
                *title_auto,
                *updated_at,
                *active_leaf,
            )
            .await
        }
        Change::DeleteSession(id) => delete_session(connection, id).await,
        Change::InsertMessage(message) => insert_message(connection, message).await,
        Change::ReplaceMessageParts { id, parts } => {
            replace_message_parts(connection, *id, parts).await
        }
        Change::BindMessageRun { id, run_id } => bind_message_run(connection, *id, *run_id).await,
        Change::RebindMessageParent { id, parent_id } => {
            rebind_message_parent(connection, *id, *parent_id).await
        }
        Change::ClearPendingSteer { session, item } => {
            clear_pending_steer(connection, session, *item).await
        }
        Change::InsertQueueItem { session, item } => {
            insert_queue_item(connection, session, item).await
        }
        Change::DeleteQueueItem { session, item } => {
            delete_queue_item(connection, session, *item).await
        }
        Change::InsertRun(run) => insert_run(connection, run).await,
        Change::UpdateRun {
            id,
            status,
            failure,
            updated_at,
        } => update_run(connection, *id, *status, failure.as_ref(), *updated_at).await,
        Change::AppendEvent { event, created_at } => {
            append_event(connection, event, *created_at, committed).await
        }
    }
}

async fn insert_workspace(
    connection: &mut SqliteConnection,
    workspace: &WorkspaceMetadata,
) -> Result<(), StoreError> {
    sqlx::query("INSERT INTO workspace (path, registered_at) VALUES (?1, ?2)")
        .bind(workspace.path.as_str())
        .bind(workspace.registered_at)
        .execute(&mut *connection)
        .await?;
    Ok(())
}

async fn delete_workspace(
    connection: &mut SqliteConnection,
    path: &WorkspacePath,
) -> Result<(), StoreError> {
    let result = sqlx::query("DELETE FROM workspace WHERE path = ?1")
        .bind(path.as_str())
        .execute(&mut *connection)
        .await?;
    ensure_written(&result, "删除 workspace")
}

async fn insert_node(
    connection: &mut SqliteConnection,
    node: &NodeMetadata,
) -> Result<(), StoreError> {
    sqlx::query(
        "INSERT INTO node (name, url, enabled, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )
    .bind(node.name.as_str())
    .bind(&node.url)
    .bind(node.enabled)
    .bind(node.created_at)
    .bind(node.updated_at)
    .execute(&mut *connection)
    .await?;
    Ok(())
}

async fn update_node(
    connection: &mut SqliteConnection,
    node: &NodeMetadata,
) -> Result<(), StoreError> {
    let result =
        sqlx::query("UPDATE node SET url = ?2, enabled = ?3, updated_at = ?4 WHERE name = ?1")
            .bind(node.name.as_str())
            .bind(&node.url)
            .bind(node.enabled)
            .bind(node.updated_at)
            .execute(&mut *connection)
            .await?;
    ensure_written(&result, "更新 node")
}

async fn delete_node(connection: &mut SqliteConnection, name: &NodeName) -> Result<(), StoreError> {
    let result = sqlx::query("DELETE FROM node WHERE name = ?1")
        .bind(name.as_str())
        .execute(&mut *connection)
        .await?;
    ensure_written(&result, "删除 node")
}

async fn insert_session(
    connection: &mut SqliteConnection,
    session: &SessionRecord,
) -> Result<(), StoreError> {
    sqlx::query(
        "INSERT INTO session (id, workspace, title, title_auto, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )
    .bind(session.id.as_str())
    .bind(session.workspace.as_str())
    .bind(session.title.as_str())
    .bind(session.title_auto)
    .bind(session.created_at)
    .bind(session.updated_at)
    .execute(&mut *connection)
    .await?;
    Ok(())
}

async fn update_session(
    connection: &mut SqliteConnection,
    id: &SessionId,
    title: &SessionTitle,
    title_auto: bool,
    updated_at: i64,
    active_leaf: Option<MessageId>,
) -> Result<(), StoreError> {
    let result = sqlx::query(
        "UPDATE session SET title = ?2, title_auto = ?3, updated_at = ?4, active_leaf = ?5 WHERE id = ?1",
    )
    .bind(id.as_str())
    .bind(title.as_str())
    .bind(title_auto)
    .bind(updated_at)
    .bind(active_leaf.map(MessageId::get))
    .execute(&mut *connection)
    .await?;
    ensure_written(&result, "更新 session")
}

async fn delete_session(
    connection: &mut SqliteConnection,
    id: &SessionId,
) -> Result<(), StoreError> {
    for statement in [
        "DELETE FROM event WHERE session_id = ?1",
        "DELETE FROM queue_item WHERE session_id = ?1",
        "DELETE FROM message_part WHERE message_id IN (SELECT id FROM message WHERE session_id = ?1)",
        "DELETE FROM message WHERE session_id = ?1",
        "DELETE FROM run WHERE session_id = ?1",
    ] {
        sqlx::query(statement)
            .bind(id.as_str())
            .execute(&mut *connection)
            .await?;
    }
    let result = sqlx::query("DELETE FROM session WHERE id = ?1")
        .bind(id.as_str())
        .execute(&mut *connection)
        .await?;
    ensure_written(&result, "删除 session")
}

async fn insert_message(
    connection: &mut SqliteConnection,
    message: &Message,
) -> Result<(), StoreError> {
    sqlx::query(
        "INSERT INTO message (id, session_id, parent_id, role, run_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )
    .bind(message.node.id.get())
    .bind(message.session.as_str())
    .bind(message.node.parent_id.map(MessageId::get))
    .bind(message.node.role.as_str())
    .bind(message.node.run_id.map(RunId::get))
    .bind(message.node.created_at)
    .execute(&mut *connection)
    .await?;
    insert_parts(connection, message.node.id, &message.node.parts).await
}

async fn replace_message_parts(
    connection: &mut SqliteConnection,
    id: MessageId,
    parts: &[ContentPart],
) -> Result<(), StoreError> {
    sqlx::query("DELETE FROM message_part WHERE message_id = ?1")
        .bind(id.get())
        .execute(&mut *connection)
        .await?;
    insert_parts(connection, id, parts).await
}

async fn insert_parts(
    connection: &mut SqliteConnection,
    id: MessageId,
    parts: &[ContentPart],
) -> Result<(), StoreError> {
    for (seq, part) in parts.iter().enumerate() {
        let seq = i64::try_from(seq)
            .map_err(|_| StoreError::Invariant("消息块序号超出 i64".to_owned()))?;
        sqlx::query(
            "INSERT INTO message_part (message_id, seq, kind, state, body)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )
        .bind(id.get())
        .bind(seq)
        .bind(part.kind().as_str())
        .bind(part.state().as_str())
        .bind(serde_json::to_string(part)?)
        .execute(&mut *connection)
        .await?;
    }
    Ok(())
}

async fn bind_message_run(
    connection: &mut SqliteConnection,
    id: MessageId,
    run_id: RunId,
) -> Result<(), StoreError> {
    let result = sqlx::query("UPDATE message SET run_id = ?2 WHERE id = ?1")
        .bind(id.get())
        .bind(run_id.get())
        .execute(&mut *connection)
        .await?;
    ensure_written(&result, "绑定消息 run")
}

async fn rebind_message_parent(
    connection: &mut SqliteConnection,
    id: MessageId,
    parent_id: Option<MessageId>,
) -> Result<(), StoreError> {
    let result = sqlx::query("UPDATE message SET parent_id = ?2 WHERE id = ?1")
        .bind(id.get())
        .bind(parent_id.map(MessageId::get))
        .execute(&mut *connection)
        .await?;
    ensure_written(&result, "改写消息父节点")
}

async fn insert_queue_item(
    connection: &mut SqliteConnection,
    session: &SessionId,
    item: &QueueItem,
) -> Result<(), StoreError> {
    sqlx::query(
        "INSERT INTO queue_item (id, session_id, text, anchor, created_at, pending_steer)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )
    .bind(item.id.get())
    .bind(session.as_str())
    .bind(item.text.as_str())
    .bind(item.anchor.map(MessageId::get))
    .bind(item.created_at)
    .bind(item.pending_steer)
    .execute(&mut *connection)
    .await?;
    Ok(())
}

async fn clear_pending_steer(
    connection: &mut SqliteConnection,
    session: &SessionId,
    item: QueueItemId,
) -> Result<(), StoreError> {
    let result =
        sqlx::query("UPDATE queue_item SET pending_steer = 0 WHERE id = ?1 AND session_id = ?2")
            .bind(item.get())
            .bind(session.as_str())
            .execute(&mut *connection)
            .await?;
    ensure_written(&result, "清除待插入标记")
}

async fn delete_queue_item(
    connection: &mut SqliteConnection,
    session: &SessionId,
    item: QueueItemId,
) -> Result<(), StoreError> {
    let result = sqlx::query("DELETE FROM queue_item WHERE id = ?1 AND session_id = ?2")
        .bind(item.get())
        .bind(session.as_str())
        .execute(&mut *connection)
        .await?;
    ensure_written(&result, "删除队列项")
}

async fn insert_run(connection: &mut SqliteConnection, run: &RunRecord) -> Result<(), StoreError> {
    sqlx::query(
        "INSERT INTO run (id, session_id, status, failure, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )
    .bind(run.id.get())
    .bind(run.session_id.as_str())
    .bind(run.status.as_str())
    .bind(
        run.failure
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?,
    )
    .bind(run.created_at)
    .bind(run.updated_at)
    .execute(&mut *connection)
    .await?;
    Ok(())
}

async fn update_run(
    connection: &mut SqliteConnection,
    id: RunId,
    status: RunStatus,
    failure: Option<&RunFailure>,
    updated_at: i64,
) -> Result<(), StoreError> {
    let result =
        sqlx::query("UPDATE run SET status = ?2, failure = ?3, updated_at = ?4 WHERE id = ?1")
            .bind(id.get())
            .bind(status.as_str())
            .bind(failure.map(serde_json::to_string).transpose()?)
            .bind(updated_at)
            .execute(&mut *connection)
            .await?;
    ensure_written(&result, "更新 run")
}

async fn append_event(
    connection: &mut SqliteConnection,
    event: &FactEvent,
    created_at: i64,
    committed: &mut Vec<CommittedEvent>,
) -> Result<(), StoreError> {
    let result =
        sqlx::query("INSERT INTO event (session_id, created_at, event) VALUES (?1, ?2, ?3)")
            .bind(event.session_id().map(SessionId::as_str))
            .bind(created_at)
            .bind(serde_json::to_string(event)?)
            .execute(&mut *connection)
            .await?;
    committed.push(CommittedEvent {
        id: result.last_insert_rowid(),
        event: event.clone(),
        created_at,
    });
    Ok(())
}

/// 写入线保证变更命中已读取的行；零行写入说明不变量已被破坏。
fn ensure_written(result: &SqliteQueryResult, operation: &str) -> Result<(), StoreError> {
    if result.rows_affected() == 0 {
        return Err(StoreError::Invariant(format!("{operation} 未命中任何行")));
    }
    Ok(())
}
