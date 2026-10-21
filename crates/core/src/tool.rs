//! 工具调用的持久记录、结果与错误码。
//! 工具实现与路径边界在 `crates/agent/src/tools`。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 工具失败的稳定类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ToolErrorCode {
    UnknownTool,
    InvalidArguments,
    PathEscape,
    FileNotFound,
    NotUtf8,
    ReadLimitExceeded,
    InvalidLineRange,
    ParentNotFound,
    EditNotFound,
    EditNotUnique,
    BashTimeout,
    IoError,
    Cancelled,
}

/// 工具调用的结局。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "status",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ToolOutcome {
    Succeeded {
        content: String,
    },
    Failed {
        code: ToolErrorCode,
        message: String,
    },
}

/// assistant 消息发起的单个工具调用及其结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallRecord {
    pub id: String,
    pub name: String,
    pub arguments: String,
    pub outcome: ToolOutcome,
}
