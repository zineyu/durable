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
use crate::event::{CommittedEvent, FactEvent};
use crate::id::{MessageId, QueueItemId, RunId, SessionId, SessionTitle, WorkspacePath};
use crate::message::{Message, MessageContent, MessageNode, MessageRole};
use crate::queue::QueueItem;
use crate::run::{RunFailure, RunRecord, RunStatus};
use crate::session::SessionRecord;
use crate::storage::{SessionSnapshot, Snapshot};
use crate::tool::ToolCallRecord;
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
        let max_event_id: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(id), 0) FROM event")
            .fetch_one(&mut *transaction)
            .await?;
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
                    (SELECT COUNT(*) FROM queue_item WHERE session_id = session.id) AS queue_length,
                    (SELECT COUNT(*) FROM run WHERE session_id = session.id AND status = 'running') AS running_count
             FROM session ORDER BY updated_at DESC, rowid DESC",
        )
        .fetch_all(&mut *transaction)
        .await?
        .iter()
        .map(parse_session_metadata)
        .collect::<Result<Vec<_>, _>>()?;
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

    /// session 当前是否有 running 的 run。
    pub async fn busy(&self, id: &SessionId) -> Result<bool, StoreError> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM run WHERE session_id = ?1 AND status = 'running'",
        )
        .bind(id.as_str())
        .fetch_one(&self.pool)
        .await?;
        Ok(count > 0)
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
        let max_event_id: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(id), 0) FROM event")
            .fetch_one(&mut *transaction)
            .await?;
        let row = sqlx::query(
            "SELECT id, workspace, title, title_auto, created_at, updated_at, active_leaf,
                    (SELECT COUNT(*) FROM queue_item WHERE session_id = session.id) AS queue_length,
                    (SELECT COUNT(*) FROM run WHERE session_id = session.id AND status = 'running') AS running_count
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
        let busy = parse_busy(&row)?;
        let messages = sqlx::query(
            "SELECT id, parent_id, role, text, run_id, tool_calls, created_at FROM message
             WHERE session_id = ?1 ORDER BY id ASC",
        )
        .bind(id.as_str())
        .fetch_all(&mut *transaction)
        .await?
        .iter()
        .map(|row| parse_message(id, row))
        .collect::<Result<Vec<_>, _>>()?;
        let queue = sqlx::query(
            "SELECT id, text, anchor, created_at FROM queue_item WHERE session_id = ?1 ORDER BY id ASC",
        )
        .bind(id.as_str())
        .fetch_all(&mut *transaction)
        .await?
        .iter()
        .map(parse_queue_item)
        .collect::<Result<Vec<_>, _>>()?;
        let runs = sqlx::query(
            "SELECT id, session_id, status, failure, created_at, updated_at FROM run
             WHERE session_id = ?1 ORDER BY id ASC",
        )
        .bind(id.as_str())
        .fetch_all(&mut *transaction)
        .await?
        .iter()
        .map(parse_run)
        .collect::<Result<Vec<_>, _>>()?;
        transaction.commit().await?;
        Ok(Some(SessionSnapshot {
            session,
            busy,
            queue_length,
            messages,
            queue,
            runs,
            max_event_id,
        }))
    }

    /// session 的最早队列项。
    pub async fn first_queue_item(&self, id: &SessionId) -> Result<Option<QueueItem>, StoreError> {
        sqlx::query(
            "SELECT id, text, anchor, created_at FROM queue_item WHERE session_id = ?1 ORDER BY id ASC LIMIT 1",
        )
        .bind(id.as_str())
        .fetch_optional(&self.pool)
        .await?
        .as_ref()
        .map(parse_queue_item)
        .transpose()
    }

    /// 指定 session 中的单个队列项。
    pub async fn queue_item(
        &self,
        session: &SessionId,
        id: QueueItemId,
    ) -> Result<Option<QueueItem>, StoreError> {
        sqlx::query(
            "SELECT id, text, anchor, created_at FROM queue_item WHERE session_id = ?1 AND id = ?2",
        )
        .bind(session.as_str())
        .bind(id.get())
        .fetch_optional(&self.pool)
        .await?
        .as_ref()
        .map(parse_queue_item)
        .transpose()
    }

    /// 指定 session 中的单个消息节点。
    pub async fn message(
        &self,
        session: &SessionId,
        id: MessageId,
    ) -> Result<Option<Message>, StoreError> {
        sqlx::query(
            "SELECT id, parent_id, role, text, run_id, tool_calls, created_at FROM message
             WHERE session_id = ?1 AND id = ?2",
        )
        .bind(session.as_str())
        .bind(id.get())
        .fetch_optional(&self.pool)
        .await?
        .as_ref()
        .map(|row| parse_message(session, row))
        .transpose()
    }

    /// 全部仍为 running 的 run，按 id 升序。
    pub async fn running_runs(&self) -> Result<Vec<RunRecord>, StoreError> {
        let rows = sqlx::query(
            "SELECT id, session_id, status, failure, created_at, updated_at FROM run
             WHERE status = 'running' ORDER BY id ASC",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(parse_run).collect()
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

    /// 所有队列非空的 session，按最近更新排序。
    pub async fn sessions_with_queue(&self) -> Result<Vec<SessionId>, StoreError> {
        let rows = sqlx::query(
            "SELECT DISTINCT s.id AS id, s.updated_at AS updated_at
             FROM session s JOIN queue_item q ON q.session_id = s.id
             ORDER BY s.updated_at ASC",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| {
                SessionId::parse(&row.get::<String, _>("id"))
                    .map_err(|error| corrupted("session.id", error))
            })
            .collect()
    }

    pub async fn apply(&self, changes: &Changeset) -> Result<Vec<CommittedEvent>, StoreError> {
        let mut transaction = self.pool.begin().await?;
        let mut committed = Vec::new();
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
                    for statement in [
                        "DELETE FROM event WHERE session_id = ?1",
                        "DELETE FROM queue_item WHERE session_id = ?1",
                        "DELETE FROM message WHERE session_id = ?1",
                        "DELETE FROM run WHERE session_id = ?1",
                    ] {
                        sqlx::query(statement)
                            .bind(id.as_str())
                            .execute(&mut *transaction)
                            .await?;
                    }
                    let result = sqlx::query("DELETE FROM session WHERE id = ?1")
                        .bind(id.as_str())
                        .execute(&mut *transaction)
                        .await?;
                    ensure_written(&result, "删除 session")?;
                }
                Change::InsertMessage(message) => {
                    sqlx::query(
                        "INSERT INTO message (id, session_id, parent_id, role, text, run_id, tool_calls, created_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    )
                    .bind(message.node.id.get())
                    .bind(message.session.as_str())
                    .bind(message.node.parent_id.map(MessageId::get))
                    .bind(message.node.role.as_str())
                    .bind(message.node.text.as_str())
                    .bind(message.node.run_id.map(RunId::get))
                    .bind(serde_json::to_string(&message.node.tool_calls)?)
                    .bind(message.node.created_at)
                    .execute(&mut *transaction)
                    .await?;
                }
                Change::UpdateMessage {
                    id,
                    text,
                    tool_calls,
                } => {
                    let result =
                        sqlx::query("UPDATE message SET text = ?2, tool_calls = ?3 WHERE id = ?1")
                            .bind(id.get())
                            .bind(text.as_str())
                            .bind(serde_json::to_string(tool_calls)?)
                            .execute(&mut *transaction)
                            .await?;
                    ensure_written(&result, "更新消息")?;
                }
                Change::BindMessageRun { id, run_id } => {
                    let result = sqlx::query("UPDATE message SET run_id = ?2 WHERE id = ?1")
                        .bind(id.get())
                        .bind(run_id.get())
                        .execute(&mut *transaction)
                        .await?;
                    ensure_written(&result, "绑定消息 run")?;
                }
                Change::InsertQueueItem { session, item } => {
                    sqlx::query(
                        "INSERT INTO queue_item (id, session_id, text, anchor, created_at)
                         VALUES (?1, ?2, ?3, ?4, ?5)",
                    )
                    .bind(item.id.get())
                    .bind(session.as_str())
                    .bind(item.text.as_str())
                    .bind(item.anchor.map(MessageId::get))
                    .bind(item.created_at)
                    .execute(&mut *transaction)
                    .await?;
                }
                Change::DeleteQueueItem { session, item } => {
                    let result =
                        sqlx::query("DELETE FROM queue_item WHERE id = ?1 AND session_id = ?2")
                            .bind(item.get())
                            .bind(session.as_str())
                            .execute(&mut *transaction)
                            .await?;
                    ensure_written(&result, "删除队列项")?;
                }
                Change::InsertRun(run) => {
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
                    .execute(&mut *transaction)
                    .await?;
                }
                Change::UpdateRun {
                    id,
                    status,
                    failure,
                    updated_at,
                } => {
                    let result = sqlx::query(
                        "UPDATE run SET status = ?2, failure = ?3, updated_at = ?4 WHERE id = ?1",
                    )
                    .bind(id.get())
                    .bind(status.as_str())
                    .bind(failure.as_ref().map(serde_json::to_string).transpose()?)
                    .bind(updated_at)
                    .execute(&mut *transaction)
                    .await?;
                    ensure_written(&result, "更新 run")?;
                }
                Change::AppendEvent { event, created_at } => {
                    let result = sqlx::query(
                        "INSERT INTO event (session_id, created_at, event) VALUES (?1, ?2, ?3)",
                    )
                    .bind(event.session_id().map(SessionId::as_str))
                    .bind(created_at)
                    .bind(serde_json::to_string(event)?)
                    .execute(&mut *transaction)
                    .await?;
                    committed.push(CommittedEvent {
                        id: result.last_insert_rowid(),
                        event: event.clone(),
                        created_at: *created_at,
                    });
                }
            }
        }
        transaction.commit().await?;
        Ok(committed)
    }

    /// 按 id 升序回放事实事件。
    pub async fn events_after(&self, after: i64) -> Result<Vec<CommittedEvent>, StoreError> {
        let rows =
            sqlx::query("SELECT id, created_at, event FROM event WHERE id > ?1 ORDER BY id ASC")
                .bind(after)
                .fetch_all(&self.pool)
                .await?;
        rows.iter()
            .map(|row| {
                Ok(CommittedEvent {
                    id: row.get("id"),
                    event: serde_json::from_str::<FactEvent>(&row.get::<String, _>("event"))?,
                    created_at: row.get("created_at"),
                })
            })
            .collect()
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

    pub async fn mint_queue_item_id(&self) -> Result<QueueItemId, StoreError> {
        let id: i64 = sqlx::query_scalar(
            "UPDATE meta SET value = value + 1 WHERE key = 'next_queue_item_id'
             RETURNING value - 1",
        )
        .fetch_one(&self.pool)
        .await?;
        QueueItemId::parse(id).map_err(|error| corrupted("queue_item.id", error))
    }

    pub async fn mint_run_id(&self) -> Result<RunId, StoreError> {
        let id: i64 = sqlx::query_scalar(
            "UPDATE meta SET value = value + 1 WHERE key = 'next_run_id'
             RETURNING value - 1",
        )
        .fetch_one(&self.pool)
        .await?;
        RunId::parse(id).map_err(|error| corrupted("run.id", error))
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

fn parse_session_metadata(row: &SqliteRow) -> Result<crate::session::SessionMetadata, StoreError> {
    let session = parse_session(row)?;
    let queue_length = parse_queue_length(row)?;
    let busy = parse_busy(row)?;
    Ok(session.metadata(queue_length, busy))
}

fn parse_queue_length(row: &SqliteRow) -> Result<u32, StoreError> {
    let count: i64 = row.get("queue_length");
    u32::try_from(count).map_err(|_| corrupted("session.queue_length", count))
}

fn parse_busy(row: &SqliteRow) -> Result<bool, StoreError> {
    Ok(row.get::<i64, _>("running_count") > 0)
}

fn parse_queue_item(row: &SqliteRow) -> Result<QueueItem, StoreError> {
    let anchor: Option<i64> = row.get("anchor");
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
    })
}

fn parse_message(session: &SessionId, row: &SqliteRow) -> Result<Message, StoreError> {
    let parent_id: Option<i64> = row.get("parent_id");
    let run_id: Option<i64> = row.get("run_id");
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
            text: MessageContent::new(row.get::<String, _>("text")),
            run_id: run_id
                .map(RunId::parse)
                .transpose()
                .map_err(|error| corrupted("message.run_id", error))?,
            tool_calls: serde_json::from_str::<Vec<ToolCallRecord>>(
                &row.get::<String, _>("tool_calls"),
            )
            .map_err(|error| corrupted("message.tool_calls", error))?,
            created_at: row.get("created_at"),
        },
    })
}

fn parse_run(row: &SqliteRow) -> Result<RunRecord, StoreError> {
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
