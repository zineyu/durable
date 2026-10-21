use agent::BootstrapResponse;
use durable::{FactEvent, MessageId, NodeMetadata, NodeName, ResyncReason, SessionId};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// hub 对浏览器的协议版本。任何不兼容的协议改动都递增它。
pub const HUB_PROTOCOL_VERSION: u32 = 2;

/// hub 期望的 agent 协议版本。与 `agent::PROTOCOL_VERSION` 由测试保持一致。
pub const REQUIRED_AGENT_PROTOCOL_VERSION: u32 = 2;

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

/// 侧栏与 bootstrap 看到的单个节点。`bootstrap` 只有 `ok` 的节点才有。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct NodeSnapshot {
    pub name: NodeName,
    pub url: String,
    pub status: NodeStatus,
    pub detail: Option<String>,
    pub bootstrap: Option<BootstrapResponse>,
}

impl NodeSnapshot {
    #[must_use]
    pub fn ok(node: NodeMetadata, bootstrap: BootstrapResponse) -> Self {
        Self {
            name: node.name,
            url: node.url,
            status: NodeStatus::Ok,
            detail: None,
            bootstrap: Some(bootstrap),
        }
    }

    #[must_use]
    pub fn failed(node: NodeMetadata, status: NodeStatus, detail: String) -> Self {
        Self {
            name: node.name,
            url: node.url,
            status,
            detail: Some(detail),
            bootstrap: None,
        }
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
pub struct RenameNodeRequest {
    pub name: String,
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
        #[ts(type = "number")]
        timestamp: i64,
        event: FactEvent,
    },
    Delta {
        node: NodeName,
        session_id: SessionId,
        #[ts(type = "number")]
        message_id: MessageId,
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
