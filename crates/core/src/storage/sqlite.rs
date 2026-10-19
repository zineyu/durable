use std::fmt;
use std::path::Path;
use std::time::Duration;

use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteQueryResult, SqliteRow,
    SqliteSynchronous,
};
use sqlx::{Row, SqlitePool};

use crate::change::{Change, Changeset};
use crate::error::StoreError;
use crate::id::{MessageId, QueueItemId, SessionId, SessionTitle, WorkspacePath};
use crate::message::{Message, MessageNode, MessageRole, MessageText};
use crate::queue::QueueItem;
use crate::session::SessionRecord;
use crate::storage::{SessionSnapshot, Snapshot};
use crate::workspace::WorkspaceMetadata;

pub struct SqliteStorage {
    pool: SqlitePool,
}

impl SqliteStorage {
    /// 打开 SQLite 文件并应用尚未执行的迁移。
    ///
    /// 失败：数据库无法打开、连接参数被拒绝或迁移失败。
    /// 副作用：创建数据库文件与表。
    pub async fn open(path: &Path) -> Result<Self, StoreError> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Full)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }

    pub async fn snapshot(&self) -> Result<Snapshot, StoreError> {
        let mut transaction = self.pool.begin().await?;
        let workspaces = sqlx::query(
            "SELECT path, registered_at FROM workspace ORDER BY registered_at ASC, path ASC",
        )
        .fetch_all(&mut *transaction)
        .await?
        .iter()
        .map(parse_workspace)
        .collect::<Result<Vec<_>, _>>()?;
        let sessions = sqlx::query(
            "SELECT id, workspace, title, title_auto, created_at, updated_at, active_leaf,
                    (SELECT COUNT(*) FROM queue_item WHERE session_id = session.id) AS queue_length
             FROM session ORDER BY updated_at DESC, rowid DESC",
        )
        .fetch_all(&mut *transaction)
        .await?
        .iter()
        .map(|row| {
            let session = parse_session(row)?;
            parse_queue_length(row).map(|queue_length| session.metadata(queue_length))
        })
        .collect::<Result<Vec<_>, _>>()?;
        let max_event_id: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(id), 0) FROM event")
            .fetch_one(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(Snapshot {
            workspaces,
            sessions,
            max_event_id,
        })
    }

    pub async fn workspace(
        &self,
        path: &WorkspacePath,
    ) -> Result<Option<WorkspaceMetadata>, StoreError> {
        sqlx::query("SELECT path, registered_at FROM workspace WHERE path = ?1")
            .bind(path.as_str())
            .fetch_optional(&self.pool)
            .await?
            .as_ref()
            .map(parse_workspace)
            .transpose()
    }

    pub async fn session(&self, id: &SessionId) -> Result<Option<SessionRecord>, StoreError> {
        sqlx::query(
            "SELECT id, workspace, title, title_auto, created_at, updated_at, active_leaf FROM session
             WHERE id = ?1",
        )
        .bind(id.as_str())
        .fetch_optional(&self.pool)
        .await?
        .as_ref()
        .map(parse_session)
        .transpose()
    }

    pub async fn session_count(&self, workspace: &WorkspacePath) -> Result<i64, StoreError> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM session WHERE workspace = ?1")
            .bind(workspace.as_str())
            .fetch_one(&self.pool)
            .await?;
        Ok(count)
    }

    pub async fn session_tree(
        &self,
        id: &SessionId,
    ) -> Result<Option<SessionSnapshot>, StoreError> {
        let mut transaction = self.pool.begin().await?;
        let row = sqlx::query(
            "SELECT id, workspace, title, title_auto, created_at, updated_at, active_leaf,
                    (SELECT COUNT(*) FROM queue_item WHERE session_id = session.id) AS queue_length
             FROM session WHERE id = ?1",
        )
        .bind(id.as_str())
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(row) = row else {
            transaction.commit().await?;
            return Ok(None);
        };
        let session = parse_session(&row)?;
        let queue_length = parse_queue_length(&row)?;
        let messages = sqlx::query(
            "SELECT id, parent_id, role, text, created_at FROM message
             WHERE session_id = ?1 ORDER BY id ASC",
        )
        .bind(id.as_str())
        .fetch_all(&mut *transaction)
        .await?
        .iter()
        .map(|row| parse_message(id, row))
        .collect::<Result<Vec<_>, _>>()?;
        let queue = sqlx::query(
            "SELECT id, text, created_at FROM queue_item WHERE session_id = ?1 ORDER BY id ASC",
        )
        .bind(id.as_str())
        .fetch_all(&mut *transaction)
        .await?
        .iter()
        .map(parse_queue_item)
        .collect::<Result<Vec<_>, _>>()?;
        transaction.commit().await?;
        Ok(Some(SessionSnapshot {
            session,
            queue_length,
            messages,
            queue,
        }))
    }

    /// session 的队列长度。
    pub async fn queue_length(&self, id: &SessionId) -> Result<u32, StoreError> {
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM queue_item WHERE session_id = ?1")
                .bind(id.as_str())
                .fetch_one(&self.pool)
                .await?;
        u32::try_from(count)
            .map_err(|_| StoreError::Invariant(format!("队列长度超出 u32: {count}")))
    }

    /// session 的消息节点数量。
    pub async fn message_count(&self, id: &SessionId) -> Result<i64, StoreError> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM message WHERE session_id = ?1")
            .bind(id.as_str())
            .fetch_one(&self.pool)
            .await?;
        Ok(count)
    }

    pub async fn apply(&self, changes: &Changeset) -> Result<(), StoreError> {
        let mut transaction = self.pool.begin().await?;
        for change in changes.operations() {
            match change {
                Change::InsertWorkspace(workspace) => {
                    sqlx::query("INSERT INTO workspace (path, registered_at) VALUES (?1, ?2)")
                        .bind(workspace.path.as_str())
                        .bind(workspace.registered_at)
                        .execute(&mut *transaction)
                        .await?;
                }
                Change::DeleteWorkspace(path) => {
                    let result = sqlx::query("DELETE FROM workspace WHERE path = ?1")
                        .bind(path.as_str())
                        .execute(&mut *transaction)
                        .await?;
                    ensure_written(&result, "删除 workspace")?;
                }
                Change::InsertSession(session) => {
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
                    .execute(&mut *transaction)
                    .await?;
                }
                Change::UpdateSession {
                    id,
                    title,
                    title_auto,
                    updated_at,
                    active_leaf,
                } => {
                    let result = sqlx::query(
                        "UPDATE session SET title = ?2, title_auto = ?3, updated_at = ?4, active_leaf = ?5 WHERE id = ?1",
                    )
                    .bind(id.as_str())
                    .bind(title.as_str())
                    .bind(title_auto)
                    .bind(updated_at)
                    .bind(active_leaf.map(MessageId::get))
                    .execute(&mut *transaction)
                    .await?;
                    ensure_written(&result, "更新 session")?;
                }
                Change::DeleteSession(id) => {
                    sqlx::query("DELETE FROM event WHERE session_id = ?1")
                        .bind(id.as_str())
                        .execute(&mut *transaction)
                        .await?;
                    let result = sqlx::query("DELETE FROM session WHERE id = ?1")
                        .bind(id.as_str())
                        .execute(&mut *transaction)
                        .await?;
                    ensure_written(&result, "删除 session")?;
                }
                Change::InsertMessage(message) => {
                    sqlx::query(
                        "INSERT INTO message (id, session_id, parent_id, role, text, created_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    )
                    .bind(message.node.id.get())
                    .bind(message.session.as_str())
                    .bind(message.node.parent_id.map(MessageId::get))
                    .bind(message.node.role.as_str())
                    .bind(message.node.text.as_str())
                    .bind(message.node.created_at)
                    .execute(&mut *transaction)
                    .await?;
                }
                Change::InsertQueueItem { session, item } => {
                    sqlx::query(
                        "INSERT INTO queue_item (id, session_id, text, created_at)
                         VALUES (?1, ?2, ?3, ?4)",
                    )
                    .bind(item.id.get())
                    .bind(session.as_str())
                    .bind(item.text.as_str())
                    .bind(item.created_at)
                    .execute(&mut *transaction)
                    .await?;
                }
                Change::AppendEvent { event, created_at } => {
                    sqlx::query(
                        "INSERT INTO event (session_id, created_at, event) VALUES (?1, ?2, ?3)",
                    )
                    .bind(event.session_id().map(SessionId::as_str))
                    .bind(created_at)
                    .bind(serde_json::to_string(event)?)
                    .execute(&mut *transaction)
                    .await?;
                }
            }
        }
        transaction.commit().await?;
        Ok(())
    }

    pub async fn mint_queue_item_id(&self) -> Result<QueueItemId, StoreError> {
        let id: i64 = sqlx::query_scalar(
            "UPDATE meta SET value = value + 1 WHERE key = 'next_queue_item_id'
             RETURNING value - 1",
        )
        .fetch_one(&self.pool)
        .await?;
        QueueItemId::parse(id).map_err(|error| corrupted("queue_item.id", error))
    }
}

fn parse_workspace(row: &SqliteRow) -> Result<WorkspaceMetadata, StoreError> {
    Ok(WorkspaceMetadata {
        path: WorkspacePath::parse(&row.get::<String, _>("path"))
            .map_err(|error| corrupted("workspace.path", error))?,
        registered_at: row.get("registered_at"),
    })
}

fn parse_session(row: &SqliteRow) -> Result<SessionRecord, StoreError> {
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

fn parse_queue_length(row: &SqliteRow) -> Result<u32, StoreError> {
    let count: i64 = row.get("queue_length");
    u32::try_from(count).map_err(|_| corrupted("session.queue_length", count))
}

fn parse_queue_item(row: &SqliteRow) -> Result<QueueItem, StoreError> {
    Ok(QueueItem {
        id: QueueItemId::parse(row.get::<i64, _>("id"))
            .map_err(|error| corrupted("queue_item.id", error))?,
        text: MessageText::parse(&row.get::<String, _>("text"))
            .map_err(|error| corrupted("queue_item.text", error))?,
        created_at: row.get("created_at"),
    })
}

fn parse_message(session: &SessionId, row: &SqliteRow) -> Result<Message, StoreError> {
    let parent_id: Option<i64> = row.get("parent_id");
    Ok(Message {
        session: session.clone(),
        node: MessageNode {
            id: MessageId::parse(row.get::<i64, _>("id"))
                .map_err(|error| corrupted("message.id", error))?,
            parent_id: parent_id
                .map(MessageId::parse)
                .transpose()
                .map_err(|error| corrupted("message.parent_id", error))?,
            role: MessageRole::parse(&row.get::<String, _>("role"))
                .map_err(|error| corrupted("message.role", error))?,
            text: MessageText::parse(&row.get::<String, _>("text"))
                .map_err(|error| corrupted("message.text", error))?,
            created_at: row.get("created_at"),
        },
    })
}

fn corrupted(field: &str, error: impl fmt::Display) -> StoreError {
    StoreError::CorruptedData(format!("{field}: {error}"))
}

/// 写入线保证变更命中已读取的行；零行写入说明不变量已被破坏。
fn ensure_written(result: &SqliteQueryResult, operation: &str) -> Result<(), StoreError> {
    if result.rows_affected() == 0 {
        return Err(StoreError::Invariant(format!("{operation} 未命中任何行")));
    }
    Ok(())
}
