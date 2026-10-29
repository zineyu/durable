mod support;

use std::time::{Duration, Instant};

use reqwest::StatusCode;
use support::fixtures::start_fake_agent;
use support::harness::{AgentHandle, Harness};

/// 规格 user story H-23：一次 bootstrap 取得全部节点的状态与元数据。
#[tokio::test]
async fn merges_bootstrap_from_each_node() {
    let harness = Harness::start().await;
    let laptop = AgentHandle::start().await;
    laptop
        .register_workspace(laptop.directory.path().to_str().expect("临时路径是 UTF-8"))
        .await;
    let server = AgentHandle::start().await;

    harness.register_node("laptop", &laptop.url).await;
    harness.register_node("server", &server.url).await;
    harness.register_node("offline", "http://127.0.0.1:9").await;

    let laptop = harness.wait_for_status("laptop", hub::NodeStatus::Ok).await;
    let payload = laptop.bootstrap.expect("可达节点带 bootstrap");
    assert_eq!(payload.workspaces.len(), 1);
    assert_eq!(payload.model, "test-model");

    let server = harness.wait_for_status("server", hub::NodeStatus::Ok).await;
    assert_eq!(
        server
            .bootstrap
            .expect("可达节点带 bootstrap")
            .workspaces
            .len(),
        0
    );

    let offline = harness
        .wait_for_status("offline", hub::NodeStatus::Unreachable)
        .await;
    assert!(offline.bootstrap.is_none(), "不可达节点不带 bootstrap");
    assert!(offline.detail.is_some(), "不可达节点必须给出说明");

    assert_eq!(harness.bootstrap().await.nodes.len(), 3);
}

/// 规格 user story H-25：节点作用域命令只落在目标节点。
#[tokio::test]
async fn forwards_node_scoped_requests_to_that_node_only() {
    let harness = Harness::start().await;
    let laptop = AgentHandle::start().await;
    let server = AgentHandle::start().await;
    harness.register_node("laptop", &laptop.url).await;
    harness.register_node("server", &server.url).await;
    harness.wait_for_status("laptop", hub::NodeStatus::Ok).await;
    harness.wait_for_status("server", hub::NodeStatus::Ok).await;

    let path = laptop.directory.path().to_str().expect("临时路径是 UTF-8");
    let response = reqwest::Client::new()
        .post(harness.url("/api/n/laptop/workspaces"))
        .json(&serde_json::json!({ "host": "local", "path": path }))
        .send()
        .await
        .expect("经 hub 注册 workspace");
    assert_eq!(response.status(), StatusCode::CREATED);

    let laptop = harness.bootstrap().await;
    let laptop = laptop
        .nodes
        .into_iter()
        .find(|node| node.name.as_str() == "laptop")
        .expect("bootstrap 含 laptop");
    assert_eq!(
        laptop.bootstrap.expect("带 bootstrap").workspaces.len(),
        1,
        "请求应落在 laptop"
    );
    let server = harness
        .snapshot("server")
        .await
        .bootstrap
        .expect("带 bootstrap");
    assert_eq!(server.workspaces.len(), 0, "server 不应收到该请求");
}

/// 规格 user story H-27：未知节点名返回显式错误。
#[tokio::test]
async fn rejects_unknown_node() {
    let harness = Harness::start().await;

    let response = reqwest::get(harness.url("/api/n/missing/bootstrap"))
        .await
        .expect("请求未知节点");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body: hub::HubErrorResponse = response.json().await.expect("解析错误响应");
    assert_eq!(body.code, hub::HubErrorCode::NodeNotFound);
}

/// 规格 user story H-28：不可达节点的命令返回显式错误。
#[tokio::test]
async fn rejects_commands_to_unreachable_node() {
    let harness = Harness::start().await;
    harness.register_node("offline", "http://127.0.0.1:9").await;
    harness
        .wait_for_status("offline", hub::NodeStatus::Unreachable)
        .await;

    let response = reqwest::get(harness.url("/api/n/offline/bootstrap"))
        .await
        .expect("请求不可达节点");
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body: hub::HubErrorResponse = response.json().await.expect("解析错误响应");
    assert_eq!(body.code, hub::HubErrorCode::NodeUnavailable);
}

/// 规格 user story H-15：协议版本不匹配的节点状态为 incompatible，命令被拒绝。
#[tokio::test]
async fn marks_incompatible_agent_and_rejects_its_commands() {
    let harness = Harness::start().await;
    let fake = start_fake_agent(hub::REQUIRED_AGENT_PROTOCOL_VERSION + 1).await;
    harness.register_node("fake", &fake).await;

    let snapshot = harness
        .wait_for_status("fake", hub::NodeStatus::Incompatible)
        .await;
    assert!(snapshot.bootstrap.is_none());

    let response = reqwest::get(harness.url("/api/n/fake/bootstrap"))
        .await
        .expect("请求版本不符的节点");
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body: hub::HubErrorResponse = response.json().await.expect("解析错误响应");
    assert_eq!(body.code, hub::HubErrorCode::NodeUnavailable);
}

/// hub 期望的 agent 协议版本必须与 agent 实际导出的常量一致。
#[test]
fn required_agent_protocol_version_matches_agent() {
    assert_eq!(
        hub::REQUIRED_AGENT_PROTOCOL_VERSION,
        agent::PROTOCOL_VERSION,
        "hub 与 agent 的协议版本不一致"
    );
}

/// ADR 0028：节点卡住时命令得到显式的 `node_unavailable`，而不是无限等待。
#[tokio::test]
async fn times_out_hung_command_with_node_unavailable() {
    let harness = Harness::start_with_options(hub::HubOptions {
        connect_timeout: Duration::from_millis(200),
        command_timeout: Duration::from_millis(300),
        stream_idle_timeout: Duration::from_secs(5),
        backoff_base: Duration::from_millis(50),
        ..hub::HubOptions::default()
    })
    .await;
    let node = support::fixtures::scripted::ScriptedNode::start().await;
    harness.register_node("laptop", &node.url).await;
    harness.wait_for_status("laptop", hub::NodeStatus::Ok).await;

    let started = Instant::now();
    let response = reqwest::get(harness.url("/api/n/laptop/hang"))
        .await
        .expect("请求挂起的命令");
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body: hub::HubErrorResponse = response.json().await.expect("解析错误响应");
    assert_eq!(body.code, hub::HubErrorCode::NodeUnavailable);
    assert!(
        body.message.contains("命令超时"),
        "说明必须点明超时: {}",
        body.message
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "必须在超时内返回"
    );
}

/// 转发的上游事件流只受空闲超时约束：命令总超时不会切断已建立的流。
#[tokio::test]
async fn forwards_event_stream_beyond_command_timeout() {
    use futures_util::StreamExt;

    let harness = Harness::start_with_options(hub::HubOptions {
        connect_timeout: Duration::from_millis(200),
        command_timeout: Duration::from_millis(100),
        stream_idle_timeout: Duration::from_secs(2),
        backoff_base: Duration::from_millis(50),
        ..hub::HubOptions::default()
    })
    .await;
    let node = support::fixtures::scripted::ScriptedNode::start().await;
    harness.register_node("laptop", &node.url).await;
    harness.wait_for_status("laptop", hub::NodeStatus::Ok).await;

    let response = reqwest::get(harness.url("/api/n/laptop/events?after=0"))
        .await
        .expect("订阅转发的上游事件流");
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        node.after_requests().contains(&Some(0)),
        "转发必须原样带上客户端游标: {:?}",
        node.after_requests()
    );
    let mut stream = response.bytes_stream();
    let mut received = String::new();

    for id in 0..4 {
        node.publish_fact(id, support::fixtures::scripted::fact());
        while !received.contains(&format!("id\":{id}")) {
            let chunk = tokio::time::timeout(Duration::from_secs(2), stream.next())
                .await
                .expect("流在命令超时之后仍然存活")
                .expect("有数据")
                .expect("读取数据");
            received.push_str(&String::from_utf8_lossy(&chunk));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let ended = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(chunk) = stream.next().await {
            assert!(
                chunk.is_ok(),
                "转发流必须以正常结束收尾，而不是读错误: {chunk:?}"
            );
        }
    })
    .await;
    assert!(ended.is_ok(), "空闲超过超时后转发的流必须结束");
}

/// 命令在响应头之后卡住时同样按时限返回 `node_unavailable`。
#[tokio::test]
async fn times_out_command_with_stalled_body() {
    let harness = Harness::start_with_options(hub::HubOptions {
        connect_timeout: Duration::from_millis(200),
        command_timeout: Duration::from_millis(300),
        stream_idle_timeout: Duration::from_secs(5),
        backoff_base: Duration::from_millis(50),
        ..hub::HubOptions::default()
    })
    .await;
    let node = support::fixtures::scripted::ScriptedNode::start().await;
    harness.register_node("laptop", &node.url).await;
    harness.wait_for_status("laptop", hub::NodeStatus::Ok).await;

    let response = tokio::time::timeout(
        Duration::from_secs(5),
        reqwest::get(harness.url("/api/n/laptop/stall")),
    )
    .await
    .expect("转发必须在时限内结束")
    .expect("请求卡住的命令");
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body: hub::HubErrorResponse = response.json().await.expect("解析错误响应");
    assert_eq!(body.code, hub::HubErrorCode::NodeUnavailable);
    assert!(
        body.message.contains("命令超时"),
        "说明必须点明超时: {}",
        body.message
    );
}
