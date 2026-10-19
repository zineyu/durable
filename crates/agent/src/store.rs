use std::fmt;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension, Row, Transaction};

use crate::error::StoreError;
use crate::protocol::{
    FactEvent, SessionId, SessionMetadata, SessionTitle, WorkspaceMetadata, WorkspacePath,
};

const SESSION_COLUMNS: &str = "id, workspace, title, updated_at";

const MIGRATIONS: &[&str] = &["CREATE TABLE workspace (
         path TEXT PRIMARY KEY,
         registered_at INTEGER NOT NULL
     );
     CREATE TABLE session (
         id TEXT PRIMARY KEY,
         workspace TEXT NOT NULL REFERENCES workspace(path),
         title TEXT NOT NULL,
         created_at INTEGER NOT NULL,
         updated_at INTEGER NOT NULL
     );
     CREATE TABLE event (
         id INTEGER PRIMARY KEY AUTOINCREMENT,
         session_id TEXT,
         created_at INTEGER NOT NULL,
         event TEXT NOT NULL
     );"];

// 规格用户故事 6 要求 session 元数据报告活动 run 与队列长度。run 与队列切片落地前
// 两者恒为空：没有 run 行、也没有队列项，因此这就是当前事实。
const SESSION_BUSY: bool = false;
const SESSION_QUEUE_LENGTH: u32 = 0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub workspaces: Vec<WorkspaceMetadata>,
    pub sessions: Vec<SessionMetadata>,
    pub max_event_id: i64,
}

pub struct Store {
    connection: Mutex<Connection>,
}

impl Store {
    /// 打开 SQLite 并应用尚未执行的迁移。
    ///
    /// 失败：数据库无法打开或迁移失败。
    /// 副作用：创建数据库文件与表。
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let mut connection = Connection::open(path)?;
        connection.pragma_update(None, "foreign_keys", true)?;
        apply_migrations(&mut connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    /// 读取 bootstrap 元数据快照：workspace 列表、session 列表与当前最大事件 id。
    ///
    /// 输出按 `registered_at` 升序排列 workspace，按 `updated_at` 降序排列 session。
    /// 失败：数据库读取失败或存储值不符合领域约束。
    pub fn snapshot(&self) -> Result<Snapshot, StoreError> {
        let connection = self.lock();
        Ok(Snapshot {
            workspaces: read_workspaces(&connection)?,
            sessions: read_sessions(&connection)?,
            max_event_id: read_max_event_id(&connection)?,
        })
    }

    /// 注册 workspace。
    ///
    /// 输入：已规范化的 workspace 身份路径。
    /// 失败：该路径已注册。
    /// 副作用：写入 workspace 行与 `workspace_registered` 事实事件。
    pub fn register_workspace(
        &self,
        path: &WorkspacePath,
    ) -> Result<WorkspaceMetadata, StoreError> {
        let mut connection = self.lock();
        let transaction = connection.transaction()?;
        if workspace_exists(&transaction, path)? {
            return Err(StoreError::WorkspaceDuplicate(path.clone()));
        }
        let registered_at = now_millis();
        let workspace = WorkspaceMetadata {
            path: path.clone(),
            registered_at,
        };
        transaction.execute(
            "INSERT INTO workspace (path, registered_at) VALUES (?1, ?2)",
            params![workspace.path.as_str(), workspace.registered_at],
        )?;
        record_event(
            &transaction,
            &FactEvent::WorkspaceRegistered {
                workspace: workspace.clone(),
            },
            registered_at,
        )?;
        transaction.commit()?;
        Ok(workspace)
    }

    /// 移除 workspace 注册记录。
    ///
    /// 输入：存储中的 workspace 身份路径；不访问文件系统，因此目录已被删除时仍可移除。
    /// 失败：该路径未注册，或该 workspace 仍有 session。
    /// 副作用：删除 workspace 行，写入 `workspace_removed` 事实事件。
    pub fn remove_workspace(&self, path: &WorkspacePath) -> Result<(), StoreError> {
        let mut connection = self.lock();
        let transaction = connection.transaction()?;
        if !workspace_exists(&transaction, path)? {
            return Err(StoreError::WorkspaceNotFound(path.clone()));
        }
        let sessions: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM session WHERE workspace = ?1",
            [path.as_str()],
            |row| row.get(0),
        )?;
        if sessions > 0 {
            return Err(StoreError::WorkspaceNotEmpty(path.clone()));
        }
        transaction.execute("DELETE FROM workspace WHERE path = ?1", [path.as_str()])?;
        record_event(
            &transaction,
            &FactEvent::WorkspaceRemoved { path: path.clone() },
            now_millis(),
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// 注册启动参数给出的初始 workspace。
    ///
    /// 输入：已规范化的 workspace 路径列表；重复出现与已注册的路径被跳过。
    /// 失败：数据库写入失败。
    /// 副作用：为新路径写入 workspace 行与 `workspace_registered` 事实事件。
    pub fn register_initial_workspaces(&self, paths: &[WorkspacePath]) -> Result<(), StoreError> {
        for path in paths {
            if self.workspace_registered(path)? {
                continue;
            }
            self.register_workspace(path)?;
        }
        Ok(())
    }

    /// 在指定 workspace 下创建 session。
    ///
    /// 输入：已规范化的 workspace 路径与已解析的标题；标题缺省时使用默认标题。
    /// 失败：该 workspace 未注册。
    /// 副作用：写入 session 行与 `session_created` 事实事件。
    pub fn create_session(
        &self,
        workspace: &WorkspacePath,
        title: SessionTitle,
    ) -> Result<SessionMetadata, StoreError> {
        let mut connection = self.lock();
        let transaction = connection.transaction()?;
        if !workspace_exists(&transaction, workspace)? {
            return Err(StoreError::WorkspaceNotFound(workspace.clone()));
        }
        let created_at = now_millis();
        let session = SessionMetadata {
            id: SessionId::generate(),
            workspace: workspace.clone(),
            title,
            busy: SESSION_BUSY,
            queue_length: SESSION_QUEUE_LENGTH,
            updated_at: created_at,
        };
        transaction.execute(
            "INSERT INTO session (id, workspace, title, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?4)",
            params![
                session.id.as_str(),
                session.workspace.as_str(),
                session.title.as_str(),
                session.updated_at
            ],
        )?;
        record_event(
            &transaction,
            &FactEvent::SessionCreated {
                session: session.clone(),
            },
            created_at,
        )?;
        transaction.commit()?;
        Ok(session)
    }

    /// 重命名 session。
    ///
    /// 输入：session 身份与已解析的标题。
    /// 失败：session 不存在。
    /// 副作用：更新标题与 `updated_at`，写入 `session_renamed` 事实事件。
    pub fn rename_session(
        &self,
        id: &SessionId,
        title: &SessionTitle,
    ) -> Result<SessionMetadata, StoreError> {
        let mut connection = self.lock();
        let transaction = connection.transaction()?;
        let mut session = find_session(&transaction, id)?
            .ok_or_else(|| StoreError::SessionNotFound(id.clone()))?;
        let updated_at = now_millis();
        transaction.execute(
            "UPDATE session SET title = ?2, updated_at = ?3 WHERE id = ?1",
            params![id.as_str(), title.as_str(), updated_at],
        )?;
        session.title = title.clone();
        session.updated_at = updated_at;
        record_event(
            &transaction,
            &FactEvent::SessionRenamed {
                session: session.clone(),
            },
            updated_at,
        )?;
        transaction.commit()?;
        Ok(session)
    }

    /// 删除 session。
    ///
    /// 输入：session 身份。
    /// 失败：session 不存在。
    /// 副作用：删除该 session 的历史事件与 session 行，再写入 `session_deleted` 事实事件；
    /// 不访问 workspace 文件系统。
    pub fn delete_session(&self, id: &SessionId) -> Result<(), StoreError> {
        let mut connection = self.lock();
        let transaction = connection.transaction()?;
        find_session(&transaction, id)?.ok_or_else(|| StoreError::SessionNotFound(id.clone()))?;
        transaction.execute("DELETE FROM event WHERE session_id = ?1", [id.as_str()])?;
        transaction.execute("DELETE FROM session WHERE id = ?1", [id.as_str()])?;
        record_event(
            &transaction,
            &FactEvent::SessionDeleted {
                session_id: id.clone(),
            },
            now_millis(),
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// 读取单个 session 的元数据。
    ///
    /// 失败：session 不存在。
    pub fn session(&self, id: &SessionId) -> Result<SessionMetadata, StoreError> {
        let connection = self.lock();
        find_session(&connection, id)?.ok_or_else(|| StoreError::SessionNotFound(id.clone()))
    }

    fn workspace_registered(&self, path: &WorkspacePath) -> Result<bool, StoreError> {
        workspace_exists(&self.lock(), path)
    }

    fn lock(&self) -> MutexGuard<'_, Connection> {
        self.connection.lock().expect("SQLite 连接锁被污染")
    }
}

fn apply_migrations(connection: &mut Connection) -> Result<(), rusqlite::Error> {
    let applied: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    for (index, migration) in MIGRATIONS.iter().enumerate().skip(applied as usize) {
        let transaction = connection.transaction()?;
        transaction.execute_batch(migration)?;
        transaction.pragma_update(None, "user_version", (index + 1) as i64)?;
        transaction.commit()?;
    }
    Ok(())
}

fn workspace_exists(connection: &Connection, path: &WorkspacePath) -> Result<bool, StoreError> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM workspace WHERE path = ?1)",
            [path.as_str()],
            |row| row.get(0),
        )
        .map_err(StoreError::from)
}

fn read_workspaces(connection: &Connection) -> Result<Vec<WorkspaceMetadata>, StoreError> {
    let mut statement = connection.prepare(
        "SELECT path, registered_at FROM workspace ORDER BY registered_at ASC, path ASC",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    rows.map(|row| {
        let (path, registered_at) = row?;
        Ok(WorkspaceMetadata {
            path: WorkspacePath::parse(&path)
                .map_err(|error| corrupted("workspace.path", error))?,
            registered_at,
        })
    })
    .collect()
}

fn read_sessions(connection: &Connection) -> Result<Vec<SessionMetadata>, StoreError> {
    let mut statement = connection.prepare(&format!(
        "SELECT {SESSION_COLUMNS} FROM session ORDER BY updated_at DESC, rowid DESC"
    ))?;
    let rows = statement.query_map([], read_session_row)?;
    rows.map(|row| row?.into_metadata()).collect()
}

struct SessionRow {
    id: String,
    workspace: String,
    title: String,
    updated_at: i64,
}

impl SessionRow {
    fn into_metadata(self) -> Result<SessionMetadata, StoreError> {
        Ok(SessionMetadata {
            id: SessionId::parse(&self.id).map_err(|error| corrupted("session.id", error))?,
            workspace: WorkspacePath::parse(&self.workspace)
                .map_err(|error| corrupted("session.workspace", error))?,
            title: SessionTitle::parse(&self.title)
                .map_err(|error| corrupted("session.title", error))?,
            busy: SESSION_BUSY,
            queue_length: SESSION_QUEUE_LENGTH,
            updated_at: self.updated_at,
        })
    }
}

fn read_session_row(row: &Row<'_>) -> rusqlite::Result<SessionRow> {
    Ok(SessionRow {
        id: row.get(0)?,
        workspace: row.get(1)?,
        title: row.get(2)?,
        updated_at: row.get(3)?,
    })
}

fn find_session(
    connection: &Connection,
    id: &SessionId,
) -> Result<Option<SessionMetadata>, StoreError> {
    connection
        .query_row(
            &format!("SELECT {SESSION_COLUMNS} FROM session WHERE id = ?1"),
            [id.as_str()],
            read_session_row,
        )
        .optional()?
        .map(SessionRow::into_metadata)
        .transpose()
}

fn read_max_event_id(connection: &Connection) -> Result<i64, StoreError> {
    connection
        .query_row("SELECT COALESCE(MAX(id), 0) FROM event", [], |row| {
            row.get(0)
        })
        .map_err(StoreError::from)
}

fn record_event(
    transaction: &Transaction<'_>,
    event: &FactEvent,
    created_at: i64,
) -> Result<(), StoreError> {
    transaction.execute(
        "INSERT INTO event (session_id, created_at, event) VALUES (?1, ?2, ?3)",
        params![
            event.session_id().map(SessionId::as_str),
            created_at,
            serde_json::to_string(event)?
        ],
    )?;
    Ok(())
}

fn corrupted(field: &str, error: impl fmt::Display) -> StoreError {
    StoreError::CorruptedData(format!("{field}: {error}"))
}

fn now_millis() -> i64 {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("系统时间早于 Unix 纪元");
    i64::try_from(duration.as_millis()).expect("系统时间超出 i64 范围")
}
