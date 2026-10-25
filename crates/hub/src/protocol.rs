//! hub 自有协议：节点表 DTO、合并事件信封与协议版本。
//! 节点状态机在 `bus/view`，上游协议复用 `agent` 的 DTO。

use agent::BootstrapResponse;
use durable::{FactEvent, MessageId, NodeMetadata, NodeName, ResyncReason, SessionId};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// hub 对浏览器的协议版本。任何不兼容的协议改动都递增它。
///
/// 5：事实事件的信封新增 `session`，载荷不再带 `session`。
pub const HUB_PROTOCOL_VERSION: u32 = 5;

/// hub 期望的 agent 协议版本。与 `agent::PROTOCOL_VERSION` 由测试保持一致。
pub const REQUIRED_AGENT_PROTOCOL_VERSION: u32 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum HubStatus {
    Ok,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct HubHealth {
    pub status: HubStatus,
    pub version: String,
    pub protocol_version: u32,
}

/// 节点状态。只有 `ok` 的节点接受命令。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum NodeStatus {
    Ok,
    Unreachable,
    Incompatible,
}

/// 节点在 hub 进程内的计数。不落库，hub 重启即清零。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NodeMetrics {
    /// 最后一条事实事件自带的时间戳。
    pub last_event_at: Option<i64>,
    /// 最近一次 health 的往返延迟。
    pub latency_ms: Option<u64>,
}

/// 侧栏与 bootstrap 看到的单个节点。`bootstrap` 只有 `ok` 的节点才有。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct NodeSnapshot {
    pub name: NodeName,
    pub url: String,
    /// 停用的节点不建上游订阅，命令一律返回不可用。
    pub enabled: bool,
    pub status: NodeStatus,
    pub detail: Option<String>,
    /// 最后一条事实事件自带的时间戳。进程内计数，不落库。
    #[ts(type = "number | null")]
    pub last_event_at: Option<i64>,
    /// 最近一次 health 的往返延迟。进程内计数，不落库。
    #[ts(type = "number | null")]
    pub latency_ms: Option<u64>,
    pub bootstrap: Option<BootstrapResponse>,
}

impl NodeSnapshot {
    #[must_use]
    pub fn ok(node: NodeMetadata, bootstrap: BootstrapResponse) -> Self {
        Self {
            bootstrap: Some(bootstrap),
            ..Self::of_view(node, NodeStatus::Ok, None)
        }
    }

    #[must_use]
    pub fn failed(node: NodeMetadata, status: NodeStatus, detail: String) -> Self {
        Self::of_view(node, status, Some(detail))
    }

    /// 按总线视图构造快照；不探测上游，`bootstrap` 为空。
    #[must_use]
    pub fn of_view(node: NodeMetadata, status: NodeStatus, detail: Option<String>) -> Self {
        Self {
            name: node.name,
            url: node.url,
            enabled: node.enabled,
            status,
            detail,
            last_event_at: None,
            latency_ms: None,
            bootstrap: None,
        }
    }

    /// 填入节点的进程内计数。
    #[must_use]
    pub fn with_metrics(mut self, metrics: NodeMetrics) -> Self {
        self.last_event_at = metrics.last_event_at;
        self.latency_ms = metrics.latency_ms;
        self
    }
}

/// 合并 bootstrap。节点级失败体现在各自的 `status` 与 `detail` 里。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct HubBootstrap {
    pub nodes: Vec<NodeSnapshot>,
    /// 当前最大广播序号。客户端用它作为续传游标的起点。
    #[ts(type = "number")]
    pub max_hub_seq: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RegisterNodeRequest {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct UpdateNodeRequest {
    /// 改名后的节点名。省略表示保留原名。
    pub name: Option<String>,
    /// 新的上游基地址。省略表示保留原 URL。
    pub url: Option<String>,
    /// 是否启用上游订阅。省略表示保留原状态。
    pub enabled: Option<bool>,
}

/// 广播流中的一条事件。带 `hub_seq` 的进入回放窗口；增量与 resync 不带。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum HubStreamEvent {
    Fact {
        #[ts(type = "number")]
        hub_seq: u64,
        node: NodeName,
        #[ts(type = "number")]
        id: i64,
        session_id: Option<SessionId>,
        /// 事实提交后的 session 视图；不属于任何 session 或删除 session 时为空。
        session: Option<durable::SessionMetadata>,
        #[ts(type = "number")]
        timestamp: i64,
        event: FactEvent,
    },
    Delta {
        node: NodeName,
        session_id: SessionId,
        #[ts(type = "number")]
        message_id: MessageId,
        kind: durable::DeltaKind,
        text: String,
        #[ts(type = "number")]
        timestamp: i64,
    },
    Retry {
        node: NodeName,
        session_id: SessionId,
        /// 即将进行的重试序号，从 1 开始。
        attempt: u32,
        /// 重试次数上限。
        max_attempts: u32,
        /// 本次等待时长。
        #[ts(type = "number")]
        delay_ms: u64,
    },
    NodeRegistered {
        #[ts(type = "number")]
        hub_seq: u64,
        node: NodeMetadata,
    },
    NodeRemoved {
        #[ts(type = "number")]
        hub_seq: u64,
        name: NodeName,
    },
    NodeUpdated {
        #[ts(type = "number")]
        hub_seq: u64,
        node: NodeMetadata,
    },
    NodeStatusChanged {
        #[ts(type = "number")]
        hub_seq: u64,
        name: NodeName,
        status: NodeStatus,
        detail: Option<String>,
    },
    Resync {
        reason: ResyncReason,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum HubErrorCode {
    NodeNameInvalid,
    NodeNameTaken,
    UnknownNode,
    NodeUnavailable,
    InvalidRequest,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct HubErrorResponse {
    pub code: HubErrorCode,
    pub message: String,
}
