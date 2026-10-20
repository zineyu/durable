use std::fmt;
use std::time::Duration;

use agent::HealthResponse;
use durable::NodeMetadata;

use crate::protocol::{NodeSnapshot, NodeStatus, REQUIRED_AGENT_PROTOCOL_VERSION};

/// 节点探测失败的原因。只携带说明，避免把整个 bootstrap 塞进 `Err`。
#[derive(Debug)]
pub enum NodeProbeError {
    Unreachable(String),
    Incompatible(String),
}

impl NodeProbeError {
    pub fn detail(&self) -> &str {
        match self {
            Self::Unreachable(detail) | Self::Incompatible(detail) => detail,
        }
    }

    fn into_snapshot(self, node: NodeMetadata) -> NodeSnapshot {
        match self {
            Self::Unreachable(detail) => {
                NodeSnapshot::failed(node, NodeStatus::Unreachable, detail)
            }
            Self::Incompatible(detail) => {
                NodeSnapshot::failed(node, NodeStatus::Incompatible, detail)
            }
        }
    }
}

impl fmt::Display for NodeProbeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.detail())
    }
}

/// 探测单个节点的健康、协议版本与 bootstrap 元数据。
///
/// 失败不返回错误：节点级失败折进 `NodeSnapshot` 的 `status` 与 `detail`。
pub async fn inspect(client: &reqwest::Client, node: NodeMetadata) -> NodeSnapshot {
    let base = base_url(&node);
    if let Err(error) = check_status(client, &base).await {
        return error.into_snapshot(node);
    }
    match fetch(client, &format!("{base}/api/bootstrap")).await {
        Ok(response) => match response.json::<agent::BootstrapResponse>().await {
            Ok(bootstrap) => NodeSnapshot::ok(node, bootstrap),
            Err(error) => NodeSnapshot::failed(
                node,
                NodeStatus::Unreachable,
                format!("bootstrap 响应无法解析: {error}"),
            ),
        },
        Err(error) => error.into_snapshot(node),
    }
}

/// 只探测健康与协议版本。`Ok(())` 表示该节点可以接受命令。
pub async fn check_status(client: &reqwest::Client, base: &str) -> Result<(), NodeProbeError> {
    let response = fetch(client, &format!("{base}/api/health")).await?;
    let health: HealthResponse = response
        .json()
        .await
        .map_err(|error| NodeProbeError::Unreachable(format!("health 响应无法解析: {error}")))?;
    if health.protocol_version != REQUIRED_AGENT_PROTOCOL_VERSION {
        return Err(NodeProbeError::Incompatible(format!(
            "agent 协议版本 {} 与 hub 要求的 {} 不一致",
            health.protocol_version, REQUIRED_AGENT_PROTOCOL_VERSION
        )));
    }
    Ok(())
}

pub fn base_url(node: &NodeMetadata) -> String {
    node.url.trim_end_matches('/').to_owned()
}

async fn fetch(client: &reqwest::Client, url: &str) -> Result<reqwest::Response, NodeProbeError> {
    let response = client
        .get(url)
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .map_err(|error| NodeProbeError::Unreachable(format!("连接节点失败: {error}")))?;
    if !response.status().is_success() {
        return Err(NodeProbeError::Unreachable(format!(
            "节点返回 {}",
            response.status()
        )));
    }
    Ok(response)
}
