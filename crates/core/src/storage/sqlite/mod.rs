//! SQLite 后端：连接、迁移、查询与共享辅助。
//! 变更落库在 `apply`，行解码在 `rows`；schema 在同级 `migrations/`。

mod apply;
mod message;
mod rows;

use self::apply::apply_change;
use self::rows::{
    parse_node, parse_queue_item, parse_run, parse_session, parse_session_metadata, parse_workspace,
};

use std::fmt;
use std::path::Path;
use std::time::Duration;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Row, SqlitePool};

use crate::change::Changeset;
use crate::error::StoreError;
use crate::event::{CommittedEvent, FactEvent};
use crate::id::{MessageId, QueueItemId, RunId, SessionId, WorkspacePath};
use crate::node::{NodeMetadata, NodeName};
use crate::queue::QueueItem;
use crate::run::RunRecord;
use crate::session::SessionRecord;
use crate::storage::Snapshot;
use crate::workspace::WorkspaceMetadata;

pub(crate) struct SqliteStorage {
    pool: SqlitePool,
}

impl SqliteStorage {
    /// 打开 SQLite 文件并应用尚未执行的迁移。
    ///
    /// 失败：数据库无法打开、连接参数被拒绝或迁移失败。
    /// 副作用：创建数据库文件与表。
    pub(crate) async fn open(path: &Path) -> Result<Self, StoreError> {
        create_parent_directory(path).await?;
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

    pub(crate) async fn snapshot(&self) -> Result<Snapshot, StoreError> {
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

    pub(crate) async fn workspace(
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

    /// 节点表中的全部节点，按注册顺序排列。
    pub(crate) async fn nodes(&self) -> Result<Vec<NodeMetadata>, StoreError> {
        sqlx::query(
            "SELECT name, url, enabled, created_at, updated_at FROM node
             ORDER BY created_at ASC, name ASC",
        )
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(parse_node)
        .collect()
    }

    pub(crate) async fn node(&self, name: &NodeName) -> Result<Option<NodeMetadata>, StoreError> {
        sqlx::query("SELECT name, url, enabled, created_at, updated_at FROM node WHERE name = ?1")
            .bind(name.as_str())
            .fetch_optional(&self.pool)
            .await?
            .as_ref()
            .map(parse_node)
            .transpose()
    }

    pub(crate) async fn session(
        &self,
        id: &SessionId,
    ) -> Result<Option<SessionRecord>, StoreError> {
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
    pub(crate) async fn busy(&self, id: &SessionId) -> Result<bool, StoreError> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM run WHERE session_id = ?1 AND status = 'running'",
        )
        .bind(id.as_str())
        .fetch_one(&self.pool)
        .await?;
        Ok(count > 0)
    }

    pub(crate) async fn session_count(&self, workspace: &WorkspacePath) -> Result<i64, StoreError> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM session WHERE workspace = ?1")
            .bind(workspace.as_str())
            .fetch_one(&self.pool)
            .await?;
        Ok(count)
    }

    /// session 的最早队列项。
    pub(crate) async fn first_queue_item(
        &self,
        id: &SessionId,
    ) -> Result<Option<QueueItem>, StoreError> {
        sqlx::query(
            "SELECT id, text, anchor, created_at, pending_steer FROM queue_item WHERE session_id = ?1 ORDER BY id ASC LIMIT 1",
        )
        .bind(id.as_str())
        .fetch_optional(&self.pool)
        .await?
        .as_ref()
        .map(parse_queue_item)
        .transpose()
    }

    /// 指定 session 中的单个队列项。
    pub(crate) async fn queue_item(
        &self,
        session: &SessionId,
        id: QueueItemId,
    ) -> Result<Option<QueueItem>, StoreError> {
        sqlx::query(
            "SELECT id, text, anchor, created_at, pending_steer FROM queue_item WHERE session_id = ?1 AND id = ?2",
        )
        .bind(session.as_str())
        .bind(id.get())
        .fetch_optional(&self.pool)
        .await?
        .as_ref()
        .map(parse_queue_item)
        .transpose()
    }

    /// session 的待插入插话，按 id 升序。
    pub(crate) async fn pending_steer_items(
        &self,
        id: &SessionId,
    ) -> Result<Vec<QueueItem>, StoreError> {
        let rows = sqlx::query(
            "SELECT id, text, anchor, created_at, pending_steer FROM queue_item
             WHERE session_id = ?1 AND pending_steer = 1 ORDER BY id ASC",
        )
        .bind(id.as_str())
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(parse_queue_item).collect()
    }

    /// 全部仍为 running 的 run，按 id 升序。
    pub(crate) async fn running_runs(&self) -> Result<Vec<RunRecord>, StoreError> {
        let rows = sqlx::query(
            "SELECT id, session_id, status, failure, created_at, updated_at FROM run
             WHERE status = 'running' ORDER BY id ASC",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(parse_run).collect()
    }

    /// session 的队列长度。
    pub(crate) async fn queue_length(&self, id: &SessionId) -> Result<u32, StoreError> {
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM queue_item WHERE session_id = ?1")
                .bind(id.as_str())
                .fetch_one(&self.pool)
                .await?;
        u32::try_from(count)
            .map_err(|_| StoreError::Invariant(format!("队列长度超出 u32: {count}")))
    }

    /// session 的消息节点数量。
    pub(crate) async fn message_count(&self, id: &SessionId) -> Result<i64, StoreError> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM message WHERE session_id = ?1")
            .bind(id.as_str())
            .fetch_one(&self.pool)
            .await?;
        Ok(count)
    }

    /// 需要执行循环的 session：有 running 的 run 或队列非空，按最近更新排序。
    pub(crate) async fn sessions_with_work(&self) -> Result<Vec<SessionId>, StoreError> {
        let rows = sqlx::query(
            "SELECT id FROM session
             WHERE EXISTS (SELECT 1 FROM queue_item WHERE queue_item.session_id = session.id)
                OR EXISTS (SELECT 1 FROM run WHERE run.session_id = session.id AND run.status = 'running')
             ORDER BY updated_at ASC",
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

    /// 该 session 仍为 running 的 run，最多一条。
    pub(crate) async fn running_run(
        &self,
        id: &SessionId,
    ) -> Result<Option<RunRecord>, StoreError> {
        let row = sqlx::query(
            "SELECT id, session_id, status, failure, created_at, updated_at FROM run
             WHERE session_id = ?1 AND status = 'running' ORDER BY id ASC LIMIT 1",
        )
        .bind(id.as_str())
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(parse_run).transpose()
    }

    /// 所有队列非空的 session，按最近更新排序。
    pub(crate) async fn sessions_with_queue(&self) -> Result<Vec<SessionId>, StoreError> {
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

    pub(crate) async fn apply(
        &self,
        changes: &Changeset,
    ) -> Result<Vec<CommittedEvent>, StoreError> {
        let mut transaction = self.pool.begin().await?;
        let mut committed = Vec::new();
        for change in changes.operations() {
            apply_change(&mut transaction, change, &mut committed).await?;
        }
        transaction.commit().await?;
        Ok(committed)
    }

    /// 按 id 升序回放事实事件。
    pub(crate) async fn events_after(&self, after: i64) -> Result<Vec<CommittedEvent>, StoreError> {
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

    pub(crate) async fn mint_message_id(&self) -> Result<MessageId, StoreError> {
        let id: i64 = sqlx::query_scalar(
            "UPDATE meta SET value = value + 1 WHERE key = 'next_message_id'
             RETURNING value - 1",
        )
        .fetch_one(&self.pool)
        .await?;
        MessageId::parse(id).map_err(|error| corrupted("message.id", error))
    }

    pub(crate) async fn mint_queue_item_id(&self) -> Result<QueueItemId, StoreError> {
        let id: i64 = sqlx::query_scalar(
            "UPDATE meta SET value = value + 1 WHERE key = 'next_queue_item_id'
             RETURNING value - 1",
        )
        .fetch_one(&self.pool)
        .await?;
        QueueItemId::parse(id).map_err(|error| corrupted("queue_item.id", error))
    }

    pub(crate) async fn mint_run_id(&self) -> Result<RunId, StoreError> {
        let id: i64 = sqlx::query_scalar(
            "UPDATE meta SET value = value + 1 WHERE key = 'next_run_id'
             RETURNING value - 1",
        )
        .fetch_one(&self.pool)
        .await?;
        RunId::parse(id).map_err(|error| corrupted("run.id", error))
    }
}

fn corrupted(field: &str, error: impl fmt::Display) -> StoreError {
    StoreError::CorruptedData(format!("{field}: {error}"))
}

/// 建立数据库文件的父目录。SQLite 只创建文件，不创建目录。
///
/// 失败：目录无法建立，例如父级不可写。
async fn create_parent_directory(path: &Path) -> Result<(), StoreError> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    if parent.as_os_str().is_empty() {
        return Ok(());
    }
    tokio::fs::create_dir_all(parent)
        .await
        .map_err(|error| StoreError::DatabaseDirectory {
            path: parent.to_path_buf(),
            message: error.to_string(),
        })
}
