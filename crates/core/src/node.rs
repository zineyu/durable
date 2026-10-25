//! 节点名与节点元数据：名字即 hub 节点的身份。
//! 节点状态机与上游订阅在 `crates/hub/src/bus`。

use std::fmt;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::id::deserialize_validated;

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

deserialize_validated!(NodeName);

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
