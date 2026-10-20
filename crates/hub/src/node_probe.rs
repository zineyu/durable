use std::time::Duration;

use agent::BootstrapResponse;
use durable::NodeMetadata;

/// 读取节点的 bootstrap 元数据。状态由总线负责，这里只管取数据。
pub async fn fetch_bootstrap(
    client: &reqwest::Client,
    node: &NodeMetadata,
) -> Result<BootstrapResponse, String> {
    let url = format!("{}/api/bootstrap", base_url(node));
    let response = client
        .get(url)
        .timeout(Duration::from_secs(5))
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

pub fn base_url(node: &NodeMetadata) -> String {
    node.url.trim_end_matches('/').to_owned()
}
