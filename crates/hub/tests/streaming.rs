mod support;

use std::time::Duration;

use futures_util::StreamExt;
use support::{AgentHandle, Harness};

/// 规格 user story 26：节点作用域的 SSE 原样透传，上游发一段就转一段。
#[tokio::test]
async fn streams_node_events_without_buffering() {
    let harness = Harness::start().await;
    let laptop = AgentHandle::start().await;
    harness.register_node("laptop", &laptop.url).await;

    let upstream = reqwest::get(harness.url("/api/n/laptop/events"))
        .await
        .expect("订阅节点事件流");
    assert_eq!(upstream.status(), reqwest::StatusCode::OK);
    let mut stream = upstream.bytes_stream();

    let path = laptop.directory.path().to_str().expect("临时路径是 UTF-8");
    reqwest::Client::new()
        .post(harness.url("/api/n/laptop/workspaces"))
        .json(&serde_json::json!({ "path": path }))
        .send()
        .await
        .expect("注册 workspace 以产生事实事件");

    let event = next_event(&mut stream).await;
    assert!(
        event.contains("workspace_registered"),
        "事件流未转发 workspace_registered: {event}"
    );
}

async fn next_event(
    stream: &mut (impl StreamExt<Item = reqwest::Result<bytes::Bytes>> + Unpin),
) -> String {
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
    tokio::time::timeout(Duration::from_secs(10), read)
        .await
        .expect("等待事件超时")
}
