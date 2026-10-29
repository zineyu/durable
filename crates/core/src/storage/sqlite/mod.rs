//! SQLite 后端：连接、迁移、查询与共享辅助。
//! 变更落库在 `apply`，行解码在 `rows`；schema 在同级 `migrations/`。

mod apply;
mod message;
mod rows;

use self::apply::apply_change;
use self::rows::{
    parse_host, parse_queue_item, parse_run, parse_session, parse_session_head,
    parse_session_metadata, parse_workspace,
};

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Row, SqlitePool};

use crate::change::Changeset;
use crate::error::StoreError;
use crate::event::{CommittedEvent, FactEvent};
use crate::host::{HostMetadata, HostName};
use crate::id::{MessageId, QueueItemId, RunId, SessionId, WorkspacePath};
use crate::queue::QueueItem;
use crate::run::RunRecord;
use crate::session::{SessionMetadata, SessionRecord};
use crate::storage::{SessionHead, Snapshot};
use crate::workspace::WorkspaceMetadata;

pub(crate) struct SqliteStorage {
    pool: SqlitePool,
    path: PathBuf,
}

/// 基线的迁移版本与描述。迁移记录不是恰好这一条即视为旧库。
const BASELINE_VERSION: i64 = 1;
const BASELINE_DESCRIPTION: &str = "baseline";

/// session 记录与两个轻量计数的查询。全量快照与命令入口共用，两处计数因此一致。
pub(crate) const SESSION_HEAD_SQL: &str =
    "SELECT id, workspace_host, workspace, title, title_auto, created_at, updated_at, active_leaf,
            (SELECT COUNT(*) FROM queue_item WHERE session_id = session.id) AS queue_length,
            (SELECT COUNT(*) FROM run WHERE session_id = session.id AND status = 'running') AS running_count
     FROM session WHERE id = ?1";

impl SqliteStorage {
    /// 打开 SQLite 文件，确认它是基线结构，再应用尚未执行的迁移。
    ///
    /// 失败：数据库无法打开、连接参数被拒绝、库是旧结构或迁移失败。旧库不迁移、
    /// 不导入，调用方必须改用新的数据库路径。
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
        verify_baseline(&pool, path).await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self {
            pool,
            path: path.to_path_buf(),
        })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) async fn hosts(&self) -> Result<Vec<HostMetadata>, StoreError> {
        sqlx::query(
            "SELECT name, kind, target, enabled, created_at, updated_at FROM host
             ORDER BY created_at ASC, name ASC",
        )
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(parse_host)
        .collect()
    }

    pub(crate) async fn host(&self, name: &HostName) -> Result<Option<HostMetadata>, StoreError> {
        sqlx::query(
            "SELECT name, kind, target, enabled, created_at, updated_at FROM host WHERE name = ?1",
        )
        .bind(name.as_str())
        .fetch_optional(&self.pool)
        .await?
        .as_ref()
        .map(parse_host)
        .transpose()
    }

    /// 该 host 上已安装的 executor 构建 id。从未安装时为 `None`。
    pub(crate) async fn host_installed_build(
        &self,
        name: &HostName,
    ) -> Result<Option<String>, StoreError> {
        let build: Option<Option<String>> =
            sqlx::query_scalar("SELECT installed_build FROM host WHERE name = ?1")
                .bind(name.as_str())
                .fetch_optional(&self.pool)
                .await?;
        Ok(build.flatten())
    }

    /// 该 host 名下已登记的 workspace 数量。
    pub(crate) async fn host_workspace_count(&self, name: &HostName) -> Result<i64, StoreError> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM workspace WHERE host = ?1")
            .bind(name.as_str())
            .fetch_one(&self.pool)
            .await?;
        Ok(count)
    }

    pub(crate) async fn snapshot(&self) -> Result<Snapshot, StoreError> {
        let mut transaction = self.pool.begin().await?;
        let max_event_id: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(id), 0) FROM event")
            .fetch_one(&mut *transaction)
            .await?;
        let hosts = sqlx::query(
            "SELECT name, kind, target, enabled, created_at, updated_at FROM host
             ORDER BY created_at ASC, name ASC",
        )
        .fetch_all(&mut *transaction)
        .await?
        .iter()
        .map(parse_host)
        .collect::<Result<Vec<_>, _>>()?;
        let workspaces = sqlx::query(
            "SELECT host, path, registered_at FROM workspace
             ORDER BY registered_at ASC, host ASC, path ASC",
        )
        .fetch_all(&mut *transaction)
        .await?
        .iter()
        .map(parse_workspace)
        .collect::<Result<Vec<_>, _>>()?;
        let sessions = sqlx::query(
            "SELECT id, workspace_host, workspace, title, title_auto, created_at, updated_at, active_leaf,
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
            hosts,
            workspaces,
            sessions,
            max_event_id,
        })
    }

    pub(crate) async fn workspace(
        &self,
        host: &HostName,
        path: &WorkspacePath,
    ) -> Result<Option<WorkspaceMetadata>, StoreError> {
        sqlx::query("SELECT host, path, registered_at FROM workspace WHERE host = ?1 AND path = ?2")
            .bind(host.as_str())
            .bind(path.as_str())
            .fetch_optional(&self.pool)
            .await?
            .as_ref()
            .map(parse_workspace)
            .transpose()
    }

    pub(crate) async fn session(
        &self,
        id: &SessionId,
    ) -> Result<Option<SessionRecord>, StoreError> {
        sqlx::query(
            "SELECT id, workspace_host, workspace, title, title_auto, created_at, updated_at, active_leaf
             FROM session WHERE id = ?1",
        )
        .bind(id.as_str())
        .fetch_optional(&self.pool)
        .await?
        .as_ref()
        .map(parse_session)
        .transpose()
    }

    /// 命令入口的轻量快照：记录、队列长度与是否有 running 的 run。
    pub(crate) async fn session_head(
        &self,
        id: &SessionId,
    ) -> Result<Option<SessionHead>, StoreError> {
        let row = sqlx::query(SESSION_HEAD_SQL)
            .bind(id.as_str())
            .fetch_optional(&self.pool)
            .await?;
        row.as_ref().map(parse_session_head).transpose()
    }

    /// 某个 workspace（host 与路径这一对）下的 session 数量。
    pub(crate) async fn session_count(
        &self,
        host: &HostName,
        workspace: &WorkspacePath,
    ) -> Result<i64, StoreError> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM session WHERE workspace_host = ?1 AND workspace = ?2",
        )
        .bind(host.as_str())
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
            "SELECT id, session_id, status, failure, progress, created_at, updated_at FROM run
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
        session: Option<&SessionMetadata>,
    ) -> Result<Vec<CommittedEvent>, StoreError> {
        let mut transaction = self.pool.begin().await?;
        let mut committed = Vec::new();
        for change in changes.operations() {
            apply_change(&mut transaction, change, session, &mut committed).await?;
        }
        transaction.commit().await?;
        Ok(committed)
    }

    /// 按 id 升序回放事实事件。
    pub(crate) async fn events_after(&self, after: i64) -> Result<Vec<CommittedEvent>, StoreError> {
        let rows = sqlx::query(
            "SELECT id, session, created_at, event FROM event WHERE id > ?1 ORDER BY id ASC",
        )
        .bind(after)
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| {
                let session: Option<String> = row.get("session");
                Ok(CommittedEvent {
                    id: row.get("id"),
                    event: serde_json::from_str::<FactEvent>(&row.get::<String, _>("event"))?,
                    session: session
                        .as_deref()
                        .map(serde_json::from_str::<SessionMetadata>)
                        .transpose()?,
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

/// 确认库是基线结构。旧库（有节点表、缺 host 表、workspace 缺 host 列、迁移记录不是基线）
/// 一律以 [`StoreError::LegacyDatabase`] 拒绝：不迁移，也不导入。
async fn verify_baseline(pool: &SqlitePool, path: &Path) -> Result<(), StoreError> {
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
    )
    .fetch_all(pool)
    .await?;
    if tables.is_empty() {
        return Ok(());
    }
    let legacy = || StoreError::LegacyDatabase(path.to_path_buf());
    let has = |name: &str| tables.iter().any(|table| table == name);
    if !has("host") || has("node") || !has("_sqlx_migrations") || !has("workspace") {
        return Err(legacy());
    }
    let applied: Vec<(i64, String)> =
        sqlx::query_as("SELECT version, description FROM _sqlx_migrations ORDER BY version")
            .fetch_all(pool)
            .await?;
    if applied != [(BASELINE_VERSION, BASELINE_DESCRIPTION.to_owned())] {
        return Err(legacy());
    }
    let columns: Vec<String> =
        sqlx::query_scalar("SELECT name FROM pragma_table_info('workspace')")
            .fetch_all(pool)
            .await?;
    if !columns.iter().any(|column| column == "host") {
        return Err(legacy());
    }
    Ok(())
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
