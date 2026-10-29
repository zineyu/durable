//! host 名、host 种类、host 元数据与 host 状态：host 名即身份。
//! host 的运行状态与 link 在 `crates/agent` 的 host registry；落库在 `store/host`。

use std::fmt;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::id::deserialize_validated;

/// host 名。名称即身份，也是 URL 路径段，因此限制为路径安全字符。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, TS)]
pub struct HostName(String);

impl HostName {
    /// 解析 host 名。
    ///
    /// # Errors
    ///
    /// - `HostNameError::Empty`：名称为空。
    /// - `HostNameError::TooLong`：名称超过 64 字节。
    /// - `HostNameError::Invalid`：名称是 `.` 或 `..`，或含非路径安全字符。
    pub fn parse(raw: &str) -> Result<Self, HostNameError> {
        if raw.is_empty() {
            return Err(HostNameError::Empty);
        }
        if raw.len() > 64 {
            return Err(HostNameError::TooLong(raw.to_owned()));
        }
        if raw == "." || raw == ".." {
            return Err(HostNameError::Invalid(raw.to_owned()));
        }
        let path_safe = raw.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        });
        if !path_safe {
            return Err(HostNameError::Invalid(raw.to_owned()));
        }
        Ok(Self(raw.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for HostName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

deserialize_validated!(HostName);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostNameError {
    Empty,
    TooLong(String),
    Invalid(String),
}

impl fmt::Display for HostNameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(formatter, "host 名不能为空"),
            Self::TooLong(raw) => write!(formatter, "host 名超过 64 字节: {raw}"),
            Self::Invalid(raw) => {
                write!(formatter, "host 名只能是字母、数字、`-`、`_`、`.`: {raw}")
            }
        }
    }
}

impl std::error::Error for HostNameError {}

/// host 的传输种类。本机 host 直接拉起子进程，ssh host 经 `ssh` 子进程。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum HostKind {
    Local,
    Ssh,
}

impl HostKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Ssh => "ssh",
        }
    }

    /// 从存储值还原。
    ///
    /// # Errors
    ///
    /// - `HostKindError`：存储值不是 `local` 或 `ssh`。
    pub fn parse(raw: &str) -> Result<Self, HostKindError> {
        match raw {
            "local" => Ok(Self::Local),
            "ssh" => Ok(Self::Ssh),
            other => Err(HostKindError(other.to_owned())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostKindError(String);

impl fmt::Display for HostKindError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "未知的 host 种类: {}", self.0)
    }
}

impl std::error::Error for HostKindError {}

/// host 表中的一行。`target` 只有 `ssh` 种类才有。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct HostMetadata {
    pub name: HostName,
    pub kind: HostKind,
    /// SSH 目标字符串，交给 `~/.ssh/config` 与 ssh-agent 解析。
    pub target: Option<String>,
    /// 停用的 host 不再重连，命令一律返回不可用。
    pub enabled: bool,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number")]
    pub updated_at: i64,
}

/// host 行的局部更新。`None` 表示保留原值；`name` 是改名后的 host 名。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HostUpdate {
    pub name: Option<HostName>,
    pub target: Option<String>,
    pub enabled: Option<bool>,
}

/// host 的运行状态。它是进程状态，只经事实事件与 bootstrap 传递，不落库。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum HostStatus {
    Ok,
    Unreachable,
    Incompatible,
}

impl HostStatus {
    #[must_use]
    pub fn is_ok(self) -> bool {
        matches!(self, Self::Ok)
    }
}

#[cfg(test)]
mod tests {
    use super::{HostKind, HostName, HostNameError};

    #[test]
    fn accepts_path_safe_names() {
        for raw in ["local", "macbook-air-02", "a.b_c"] {
            assert_eq!(HostName::parse(raw).expect("合法 host 名").as_str(), raw);
        }
    }

    #[test]
    fn rejects_empty_long_and_unsafe_names() {
        assert_eq!(HostName::parse(""), Err(HostNameError::Empty));
        assert!(matches!(
            HostName::parse(&"a".repeat(65)),
            Err(HostNameError::TooLong(_))
        ));
        for raw in [".", "..", "a/b", "host name", "主机"] {
            assert!(
                matches!(HostName::parse(raw), Err(HostNameError::Invalid(_))),
                "{raw} 被拒绝"
            );
        }
    }

    #[test]
    fn host_kind_round_trips_through_its_storage_value() {
        for kind in [HostKind::Local, HostKind::Ssh] {
            assert_eq!(HostKind::parse(kind.as_str()).expect("存储值合法"), kind);
        }
        assert!(HostKind::parse("docker").is_err());
    }
}
