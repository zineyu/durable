use durable::{NodeMetadata, NodeName};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// hub 对浏览器的协议版本。任何不兼容的协议改动都递增它。
pub const HUB_PROTOCOL_VERSION: u32 = 1;

/// hub 期望的 agent 协议版本。与 `agent::PROTOCOL_VERSION` 由测试保持一致。
pub const REQUIRED_AGENT_PROTOCOL_VERSION: u32 = 1;

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
    pub bootstrap: Option<agent::BootstrapResponse>,
}

impl NodeSnapshot {
    pub fn ok(node: NodeMetadata, bootstrap: agent::BootstrapResponse) -> Self {
        Self {
            name: node.name,
            url: node.url,
            status: NodeStatus::Ok,
            detail: None,
            bootstrap: Some(bootstrap),
        }
    }

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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RegisterNodeRequest {
    pub name: String,
    pub url: String,
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
