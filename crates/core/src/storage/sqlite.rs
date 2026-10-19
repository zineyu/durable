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
use crate::id::{MessageId, SessionId, SessionTitle, WorkspacePath};
use crate::message::{Message, MessageNode, MessageRole, MessageText};
use crate::session::SessionRecord;
use crate::storage::Snapshot;
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
            "SELECT id, workspace, title, created_at, updated_at, active_leaf FROM session
             ORDER BY updated_at DESC, rowid DESC",
        )
        .fetch_all(&mut *transaction)
        .await?
        .iter()
        .map(|row| parse_session(row).map(|session| session.metadata()))
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
            "SELECT id, workspace, title, created_at, updated_at, active_leaf FROM session
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
    ) -> Result<Option<(SessionRecord, Vec<Message>)>, StoreError> {
        let mut transaction = self.pool.begin().await?;
        let session = sqlx::query(
            "SELECT id, workspace, title, created_at, updated_at, active_leaf FROM session
             WHERE id = ?1",
        )
        .bind(id.as_str())
        .fetch_optional(&mut *transaction)
        .await?
        .as_ref()
        .map(parse_session)
        .transpose()?;
        let Some(session) = session else {
            transaction.commit().await?;
            return Ok(None);
        };
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
        transaction.commit().await?;
        Ok(Some((session, messages)))
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
                        "INSERT INTO session (id, workspace, title, created_at, updated_at)
                         VALUES (?1, ?2, ?3, ?4, ?5)",
                    )
                    .bind(session.id.as_str())
                    .bind(session.workspace.as_str())
                    .bind(session.title.as_str())
                    .bind(session.created_at)
                    .bind(session.updated_at)
                    .execute(&mut *transaction)
                    .await?;
                }
                Change::UpdateSession {
                    id,
                    title,
                    updated_at,
                    active_leaf,
                } => {
                    let result = sqlx::query(
                        "UPDATE session SET title = ?2, updated_at = ?3, active_leaf = ?4 WHERE id = ?1",
                    )
                    .bind(id.as_str())
                    .bind(title.as_str())
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

    pub async fn mint_message_id(&self) -> Result<MessageId, StoreError> {
        let id: i64 = sqlx::query_scalar(
            "UPDATE meta SET value = value + 1 WHERE key = 'next_message_id'
             RETURNING value - 1",
        )
        .fetch_one(&self.pool)
        .await?;
        MessageId::parse(id).map_err(|error| corrupted("message.id", error))
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
    Ok(SessionRecord {
        id: SessionId::parse(&row.get::<String, _>("id"))
            .map_err(|error| corrupted("session.id", error))?,
        workspace: WorkspacePath::parse(&row.get::<String, _>("workspace"))
            .map_err(|error| corrupted("session.workspace", error))?,
        title: SessionTitle::parse(&row.get::<String, _>("title"))
            .map_err(|error| corrupted("session.title", error))?,
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        active_leaf: active_leaf
            .map(MessageId::parse)
            .transpose()
            .map_err(|error| corrupted("session.active_leaf", error))?,
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
