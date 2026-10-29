//! 上游探测：节点基地址与 bootstrap 取数。
//! 订阅循环在 `bus/watch`。

use std::time::Duration;

use crate::node::NodeMetadata;
use agent::BootstrapResponse;

/// 读取节点的 bootstrap 元数据。状态由总线负责，这里只管取数据。
///
/// timeout 是「发送 + 读体」的总时限：探测卡在响应体上也不会挂住调用方。
pub(crate) async fn fetch_bootstrap(
    client: &reqwest::Client,
    node: &NodeMetadata,
    timeout: Duration,
) -> Result<BootstrapResponse, String> {
    let url = format!("{}/api/bootstrap", base_url(node));
    let response = client
        .get(url)
        .timeout(timeout)
        .send()
        .await
        .map_err(|error| format!("连接节点失败: {error}"))?;
    if !response.status().is_success() {
        return Err(format!("节点返回 {}", response.status()));
    }
    response
        .json()
        .await
        .map_err(|error| format!("bootstrap 响应无法解析: {error}"))
}

pub(crate) fn base_url(node: &NodeMetadata) -> String {
    node.url.trim_end_matches('/').to_owned()
}
