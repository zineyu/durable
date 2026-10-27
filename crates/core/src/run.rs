//! run 实体：状态、失败类别与持久记录。
//! 调度与失败判定在 `crates/agent/src/engine`。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::id::{RunId, SessionId};

/// run 的生命周期状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

impl RunStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    /// 解析存储中的状态。
    ///
    /// # Errors
    ///
    /// - `RunStatusError`：取值不是已知状态。
    pub fn parse(raw: &str) -> Result<Self, RunStatusError> {
        match raw {
            "running" => Ok(Self::Running),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            other => Err(RunStatusError(other.to_owned())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunStatusError(String);

impl std::fmt::Display for RunStatusError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "未知的 run 状态: {}", self.0)
    }
}

impl std::error::Error for RunStatusError {}

/// run 失败的稳定类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum RunFailureCode {
    ModelUpstream,
    /// 供应商限流（HTTP 429 或带 `Retry-After` 的响应），重试耗尽后仍限流。
    ModelRateLimited,
    ContextLimitExceeded,
    StepLimitExceeded,
    /// 本地存储失败：引擎内任意 `durable::StoreError` 都记为此类别，典型原因是数据库、
    /// 不变量与序列化。见 ADR 0043。
    StorageFailed,
    /// 进程中断。ADR 0037 之后不再产生，保留它是为了读出历史记录里的旧值。
    Interrupted,
}

/// run 失败类别与人类可读说明。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RunFailure {
    pub code: RunFailureCode,
    pub message: String,
}

/// 单个 run 的持久状态。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RunRecord {
    #[ts(type = "number")]
    pub id: RunId,
    pub session_id: SessionId,
    pub status: RunStatus,
    pub failure: Option<RunFailure>,
    /// 流式生成中已经产生的正文。只在 run 为 `running` 时可能非空，结束或定型时清空。
    pub progress: Option<String>,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number")]
    pub updated_at: i64,
}
