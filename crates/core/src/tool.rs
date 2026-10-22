//! 工具调用的结局与错误码：执行结果、tool 节点状态与崩溃后的未知结局。
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

/// 工具执行返回的结局。
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

/// tool 节点里工具结果块的状态。
///
/// 工具在执行前入树，此时为 [`ToolResultState::Running`]；执行完毕写入结局；
/// 非幂等工具在执行中崩溃后写 [`ToolResultState::Unknown`]，表示丢失了观测。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "status",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ToolResultState {
    Running,
    Finished { outcome: ToolOutcome },
    Unknown,
}

impl From<ToolOutcome> for ToolResultState {
    fn from(outcome: ToolOutcome) -> Self {
        Self::Finished { outcome }
    }
}
