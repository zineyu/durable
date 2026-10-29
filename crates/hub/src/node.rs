//! hub 的节点表：节点名、节点行与 SQLite 读写。
//! 节点状态机与上游订阅在 `bus`，节点表命令在 `nodes`。
//!
//! hub 在 remote-ssh 改造里被 `agent` 取代（ADR 0048）。它的节点表因此不再属于
//! 领域内核，只留在 hub 自己的库里，随 hub 一起退役。

use std::fmt;
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Row, SqlitePool};
use ts_rs::TS;

/// 节点名。节点名即身份，也是 URL 路径段，因此限制为路径安全字符。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, TS)]
pub struct NodeName(String);

impl NodeName {
    /// 解析节点名。
    ///
    /// # Errors
    ///
    /// - `NodeNameError::Empty`：名称为空。
    /// - `NodeNameError::TooLong`：名称超过 64 字节。
    /// - `NodeNameError::Invalid`：名称是 `.` 或 `..`，或含非路径安全字符。
    pub fn parse(raw: &str) -> Result<Self, NodeNameError> {
        if raw.is_empty() {
            return Err(NodeNameError::Empty);
        }
        if raw.len() > 64 {
            return Err(NodeNameError::TooLong(raw.to_owned()));
        }
        if raw == "." || raw == ".." {
            return Err(NodeNameError::Invalid(raw.to_owned()));
        }
        let path_safe = raw.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        });
        if !path_safe {
            return Err(NodeNameError::Invalid(raw.to_owned()));
        }
        Ok(Self(raw.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for NodeName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

impl<'de> Deserialize<'de> for NodeName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeNameError {
    Empty,
    TooLong(String),
    Invalid(String),
}

impl fmt::Display for NodeNameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(formatter, "节点名不能为空"),
            Self::TooLong(raw) => write!(formatter, "节点名超过 64 字节: {raw}"),
            Self::Invalid(raw) => {
                write!(formatter, "节点名只能是字母、数字、`-`、`_`、`.`: {raw}")
            }
        }
    }
}

impl std::error::Error for NodeNameError {}

/// 节点表中的一行。`url` 是 hub 连接该节点 `agent` 的基地址。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct NodeMetadata {
    pub name: NodeName,
    pub url: String,
    /// 停用的节点不建上游订阅，命令一律返回不可用。
    pub enabled: bool,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number")]
    pub updated_at: i64,
}

/// 节点行的局部更新。`None` 表示保留原值；`name` 是改名后的节点名。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NodeUpdate {
    pub name: Option<NodeName>,
    pub url: Option<String>,
    pub enabled: Option<bool>,
}

#[derive(Debug)]
pub enum NodeTableError {
    NameTaken(NodeName),
    NotFound(NodeName),
    Database(sqlx::Error),
}

impl fmt::Display for NodeTableError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NameTaken(name) => write!(formatter, "节点名已注册: {name}"),
            Self::NotFound(name) => write!(formatter, "节点未注册: {name}"),
            Self::Database(error) => write!(formatter, "SQLite 错误: {error}"),
        }
    }
}

impl std::error::Error for NodeTableError {}

impl From<sqlx::Error> for NodeTableError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

/// 节点表的连接与读写。表在 hub 第一次打开时建立。
pub struct NodeTable {
    pool: SqlitePool,
}

impl NodeTable {
    /// 打开节点表。表已存在时保持原样。
    ///
    /// # Errors
    ///
    /// - [`NodeTableError::Database`]：连接或建表失败。
    pub async fn open(path: &Path) -> Result<Self, NodeTableError> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Full)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS node (
                 name TEXT PRIMARY KEY,
                 url TEXT NOT NULL,
                 enabled INTEGER NOT NULL DEFAULT 1,
                 created_at INTEGER NOT NULL,
                 updated_at INTEGER NOT NULL
             )",
        )
        .execute(&pool)
        .await?;
        Ok(Self { pool })
    }

    /// 全部节点，按注册顺序排列。
    ///
    /// # Errors
    ///
    /// - [`NodeTableError::Database`]：读取失败。
    pub async fn nodes(&self) -> Result<Vec<NodeMetadata>, NodeTableError> {
        let rows = sqlx::query(
            "SELECT name, url, enabled, created_at, updated_at FROM node
             ORDER BY created_at ASC, name ASC",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(parse_node).collect()
    }

    /// 单个节点。
    ///
    /// # Errors
    ///
    /// - [`NodeTableError::Database`]：读取失败。
    pub async fn node(&self, name: &NodeName) -> Result<Option<NodeMetadata>, NodeTableError> {
        let row = sqlx::query(
            "SELECT name, url, enabled, created_at, updated_at FROM node WHERE name = ?1",
        )
        .bind(name.as_str())
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(parse_node).transpose()
    }

    /// 注册节点。
    ///
    /// # Errors
    ///
    /// - [`NodeTableError::NameTaken`]：该名称已注册。
    /// - [`NodeTableError::Database`]：读写失败。
    pub async fn register(
        &self,
        name: NodeName,
        url: String,
    ) -> Result<NodeMetadata, NodeTableError> {
        if self.node(&name).await?.is_some() {
            return Err(NodeTableError::NameTaken(name));
        }
        let now = now_millis();
        let node = NodeMetadata {
            name,
            url,
            enabled: true,
            created_at: now,
            updated_at: now,
        };
        sqlx::query(
            "INSERT INTO node (name, url, enabled, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )
        .bind(node.name.as_str())
        .bind(&node.url)
        .bind(node.enabled)
        .bind(node.created_at)
        .bind(node.updated_at)
        .execute(&self.pool)
        .await?;
        Ok(node)
    }

    /// 更新节点：改名、换 URL、启用或停用。
    ///
    /// # Errors
    ///
    /// - [`NodeTableError::NotFound`]：当前名不在节点表中。
    /// - [`NodeTableError::NameTaken`]：改名后的新名已被使用。
    /// - [`NodeTableError::Database`]：读写失败。
    pub async fn update(
        &self,
        name: &NodeName,
        update: NodeUpdate,
    ) -> Result<NodeMetadata, NodeTableError> {
        let Some(node) = self.node(name).await? else {
            return Err(NodeTableError::NotFound(name.clone()));
        };
        let next_name = update.name.unwrap_or_else(|| node.name.clone());
        let url = update.url.unwrap_or_else(|| node.url.clone());
        let enabled = update.enabled.unwrap_or(node.enabled);
        if next_name == node.name && url == node.url && enabled == node.enabled {
            return Ok(node);
        }
        if next_name != node.name && self.node(&next_name).await?.is_some() {
            return Err(NodeTableError::NameTaken(next_name));
        }
        let updated = NodeMetadata {
            name: next_name,
            url,
            enabled,
            created_at: node.created_at,
            updated_at: now_millis(),
        };
        if updated.name == node.name {
            sqlx::query("UPDATE node SET url = ?2, enabled = ?3, updated_at = ?4 WHERE name = ?1")
                .bind(updated.name.as_str())
                .bind(&updated.url)
                .bind(updated.enabled)
                .bind(updated.updated_at)
                .execute(&self.pool)
                .await?;
        } else {
            sqlx::query("DELETE FROM node WHERE name = ?1")
                .bind(node.name.as_str())
                .execute(&self.pool)
                .await?;
            sqlx::query(
                "INSERT INTO node (name, url, enabled, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )
            .bind(updated.name.as_str())
            .bind(&updated.url)
            .bind(updated.enabled)
            .bind(updated.created_at)
            .bind(updated.updated_at)
            .execute(&self.pool)
            .await?;
        }
        Ok(updated)
    }

    /// 移除节点。
    ///
    /// # Errors
    ///
    /// - [`NodeTableError::NotFound`]：该名称不在节点表中。
    /// - [`NodeTableError::Database`]：写入失败。
    pub async fn remove(&self, name: &NodeName) -> Result<(), NodeTableError> {
        if self.node(name).await?.is_none() {
            return Err(NodeTableError::NotFound(name.clone()));
        }
        sqlx::query("DELETE FROM node WHERE name = ?1")
            .bind(name.as_str())
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

fn parse_node(row: &sqlx::sqlite::SqliteRow) -> Result<NodeMetadata, NodeTableError> {
    let name = row.get::<String, _>("name");
    Ok(NodeMetadata {
        name: NodeName::parse(&name).map_err(|error| {
            NodeTableError::Database(sqlx::Error::Decode(Box::new(std::io::Error::other(
                error.to_string(),
            ))))
        })?,
        url: row.get("url"),
        enabled: row.get("enabled"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    })
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX)
        })
}
