use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension, Row, Transaction};
use uuid::Uuid;

use crate::error::StoreError;
use crate::protocol::{FactEvent, SessionMetadata, WorkspaceMetadata};

pub const DEFAULT_SESSION_TITLE: &str = "新 session";

// run 与队列切片落地前，session 恒为空闲且队列为空。
const SESSION_BUSY: bool = false;
const SESSION_QUEUE_LENGTH: u32 = 0;

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
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let mut connection = Connection::open(path)?;
        connection.pragma_update(None, "foreign_keys", true)?;
        apply_migrations(&mut connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub fn snapshot(&self) -> Result<Snapshot, StoreError> {
        let connection = self.lock();
        Ok(Snapshot {
            workspaces: read_workspaces(&connection)?,
            sessions: read_sessions(&connection)?,
            max_event_id: read_max_event_id(&connection)?,
        })
    }

    pub fn register_workspace(&self, raw: &str) -> Result<WorkspaceMetadata, StoreError> {
        let workspace_key = workspace_key(&normalize_workspace_path(raw)?)?;
        let mut connection = self.lock();
        let transaction = connection.transaction()?;
        if workspace_exists(&transaction, &workspace_key)? {
            return Err(StoreError::WorkspaceDuplicate(workspace_key));
        }
        let registered_at = now_millis();
        let workspace = WorkspaceMetadata {
            path: workspace_key,
            registered_at,
        };
        transaction.execute(
            "INSERT INTO workspace (path, registered_at) VALUES (?1, ?2)",
            params![workspace.path, workspace.registered_at],
        )?;
        record_event(
            &transaction,
            None,
            &FactEvent::WorkspaceRegistered {
                workspace: workspace.clone(),
            },
            registered_at,
        )?;
        transaction.commit()?;
        Ok(workspace)
    }

    pub fn remove_workspace(&self, path: &str) -> Result<(), StoreError> {
        let mut connection = self.lock();
        let transaction = connection.transaction()?;
        if !workspace_exists(&transaction, path)? {
            return Err(StoreError::WorkspaceNotFound(path.to_owned()));
        }
        let sessions: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM session WHERE workspace = ?1",
            [path],
            |row| row.get(0),
        )?;
        if sessions > 0 {
            return Err(StoreError::WorkspaceNotEmpty(path.to_owned()));
        }
        transaction.execute("DELETE FROM workspace WHERE path = ?1", [path])?;
        record_event(
            &transaction,
            None,
            &FactEvent::WorkspaceRemoved {
                path: path.to_owned(),
            },
            now_millis(),
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn register_initial_workspaces(&self, paths: &[PathBuf]) -> Result<(), StoreError> {
        for path in paths {
            let raw = path
                .to_str()
                .ok_or_else(|| StoreError::WorkspacePathInvalid(path.display().to_string()))?
                .to_owned();
            let key = workspace_key(&normalize_workspace_path(&raw)?)?;
            if self.contains_workspace(&key)? {
                continue;
            }
            self.register_workspace(&key)?;
        }
        Ok(())
    }

    fn contains_workspace(&self, path: &str) -> Result<bool, StoreError> {
        workspace_exists(&self.lock(), path).map_err(StoreError::from)
    }

    pub fn create_session(
        &self,
        workspace: &str,
        title: Option<&str>,
    ) -> Result<SessionMetadata, StoreError> {
        let title = resolve_title(title)?;
        let mut connection = self.lock();
        let transaction = connection.transaction()?;
        if !workspace_exists(&transaction, workspace)? {
            return Err(StoreError::WorkspaceNotFound(workspace.to_owned()));
        }
        let created_at = now_millis();
        let session = SessionMetadata {
            id: Uuid::now_v7().to_string(),
            workspace: workspace.to_owned(),
            title,
            busy: SESSION_BUSY,
            queue_length: SESSION_QUEUE_LENGTH,
            updated_at: created_at,
        };
        transaction.execute(
            "INSERT INTO session (id, workspace, title, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?4)",
            params![
                session.id,
                session.workspace,
                session.title,
                session.updated_at
            ],
        )?;
        record_event(
            &transaction,
            Some(&session.id),
            &FactEvent::SessionCreated {
                session: session.clone(),
            },
            created_at,
        )?;
        transaction.commit()?;
        Ok(session)
    }

    pub fn rename_session(&self, id: &str, title: &str) -> Result<SessionMetadata, StoreError> {
        let title = resolve_title(Some(title))?;
        let mut connection = self.lock();
        let transaction = connection.transaction()?;
        let mut session = find_session(&transaction, id)?
            .ok_or_else(|| StoreError::SessionNotFound(id.to_owned()))?;
        let updated_at = now_millis();
        transaction.execute(
            "UPDATE session SET title = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, title, updated_at],
        )?;
        session.title = title;
        session.updated_at = updated_at;
        record_event(
            &transaction,
            Some(id),
            &FactEvent::SessionRenamed {
                session: session.clone(),
            },
            updated_at,
        )?;
        transaction.commit()?;
        Ok(session)
    }

    pub fn delete_session(&self, id: &str) -> Result<(), StoreError> {
        let mut connection = self.lock();
        let transaction = connection.transaction()?;
        find_session(&transaction, id)?
            .ok_or_else(|| StoreError::SessionNotFound(id.to_owned()))?;
        transaction.execute("DELETE FROM event WHERE session_id = ?1", [id])?;
        transaction.execute("DELETE FROM session WHERE id = ?1", [id])?;
        record_event(
            &transaction,
            Some(id),
            &FactEvent::SessionDeleted {
                session_id: id.to_owned(),
            },
            now_millis(),
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn session(&self, id: &str) -> Result<SessionMetadata, StoreError> {
        find_session(&self.lock(), id)?.ok_or_else(|| StoreError::SessionNotFound(id.to_owned()))
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

fn normalize_workspace_path(raw: &str) -> Result<PathBuf, StoreError> {
    let path = Path::new(raw);
    if !path.is_absolute() {
        return Err(StoreError::WorkspacePathInvalid(raw.to_owned()));
    }
    let canonical = std::fs::canonicalize(path)
        .map_err(|_| StoreError::WorkspacePathInvalid(raw.to_owned()))?;
    if !canonical.is_dir() {
        return Err(StoreError::WorkspacePathInvalid(raw.to_owned()));
    }
    Ok(canonical)
}

fn workspace_key(path: &Path) -> Result<String, StoreError> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| StoreError::WorkspacePathInvalid(path.display().to_string()))
}

fn workspace_exists(connection: &Connection, path: &str) -> Result<bool, rusqlite::Error> {
    connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM workspace WHERE path = ?1)",
        [path],
        |row| row.get(0),
    )
}

fn read_workspaces(connection: &Connection) -> Result<Vec<WorkspaceMetadata>, rusqlite::Error> {
    let mut statement = connection.prepare(
        "SELECT path, registered_at FROM workspace ORDER BY registered_at ASC, path ASC",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(WorkspaceMetadata {
            path: row.get(0)?,
            registered_at: row.get(1)?,
        })
    })?;
    rows.collect()
}

fn read_sessions(connection: &Connection) -> Result<Vec<SessionMetadata>, rusqlite::Error> {
    let mut statement = connection.prepare(
        "SELECT id, workspace, title, updated_at
         FROM session ORDER BY updated_at DESC, rowid DESC",
    )?;
    let rows = statement.query_map([], read_session)?;
    rows.collect()
}

fn read_session(row: &Row<'_>) -> rusqlite::Result<SessionMetadata> {
    Ok(SessionMetadata {
        id: row.get("id")?,
        workspace: row.get("workspace")?,
        title: row.get("title")?,
        busy: SESSION_BUSY,
        queue_length: SESSION_QUEUE_LENGTH,
        updated_at: row.get("updated_at")?,
    })
}

fn find_session(
    connection: &Connection,
    id: &str,
) -> Result<Option<SessionMetadata>, rusqlite::Error> {
    connection
        .query_row(
            "SELECT id, workspace, title, updated_at FROM session WHERE id = ?1",
            [id],
            read_session,
        )
        .optional()
}

fn resolve_title(title: Option<&str>) -> Result<String, StoreError> {
    match title {
        None => Ok(DEFAULT_SESSION_TITLE.to_owned()),
        Some(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                return Err(StoreError::InvalidTitle);
            }
            Ok(trimmed.to_owned())
        }
    }
}

fn read_max_event_id(connection: &Connection) -> Result<i64, rusqlite::Error> {
    connection.query_row("SELECT COALESCE(MAX(id), 0) FROM event", [], |row| {
        row.get(0)
    })
}

fn record_event(
    transaction: &Transaction<'_>,
    session_id: Option<&str>,
    event: &FactEvent,
    created_at: i64,
) -> Result<(), StoreError> {
    transaction.execute(
        "INSERT INTO event (session_id, created_at, event) VALUES (?1, ?2, ?3)",
        params![session_id, created_at, serde_json::to_string(event)?],
    )?;
    Ok(())
}

fn now_millis() -> i64 {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("系统时间早于 Unix 纪元");
    i64::try_from(duration.as_millis()).expect("系统时间超出 i64 范围")
}
