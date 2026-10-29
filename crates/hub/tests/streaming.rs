mod support;

use std::time::Duration;

use futures_util::StreamExt;
use support::harness::{AgentHandle, Harness};

/// 规格 user story H-26：节点作用域的 SSE 原样透传，上游发一段就转一段。
#[tokio::test]
async fn streams_node_events_without_buffering() {
    let harness = Harness::start().await;
    let laptop = AgentHandle::start().await;
    harness.register_node("laptop", &laptop.url).await;
    harness.wait_for_status("laptop", hub::NodeStatus::Ok).await;

    let response = reqwest::get(harness.url("/api/n/laptop/events"))
        .await
        .expect("订阅节点事件流");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let mut stream = response.bytes_stream();

    let path = laptop.directory.path().to_str().expect("临时路径是 UTF-8");
    reqwest::Client::new()
        .post(harness.url("/api/n/laptop/workspaces"))
        .json(&serde_json::json!({ "host": "local", "path": path }))
        .send()
        .await
        .expect("注册 workspace 以产生事实事件");

    let mut collected = Vec::new();
    let read = async {
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.expect("读取事件流");
            collected.extend_from_slice(&chunk);
            let text = String::from_utf8_lossy(&collected).to_string();
            if text.contains("workspace_registered") {
                return text;
            }
        }
        panic!("事件流提前结束");
    };
    let text = tokio::time::timeout(Duration::from_secs(10), read)
        .await
        .expect("等待事件超时");
    assert!(text.contains("workspace_registered"));
}
