mod support;

use reqwest::StatusCode;
use support::{AgentHandle, Harness, start_fake_agent};

/// 规格 user story 23：一次 bootstrap 取得全部节点的状态与元数据。
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

    let bootstrap = harness.bootstrap().await;
    assert_eq!(bootstrap.nodes.len(), 3);

    let laptop = harness.snapshot("laptop").await;
    assert_eq!(laptop.status, hub::NodeStatus::Ok);
    assert_eq!(laptop.detail, None);
    let payload = laptop.bootstrap.expect("可达节点带 bootstrap");
    assert_eq!(payload.workspaces.len(), 1);
    assert_eq!(payload.model, "test-model");

    let server = harness.snapshot("server").await;
    assert_eq!(server.status, hub::NodeStatus::Ok);
    assert_eq!(
        server
            .bootstrap
            .expect("可达节点带 bootstrap")
            .workspaces
            .len(),
        0
    );

    let offline = harness.snapshot("offline").await;
    assert_eq!(offline.status, hub::NodeStatus::Unreachable);
    assert!(offline.bootstrap.is_none(), "不可达节点不带 bootstrap");
    assert!(offline.detail.is_some(), "不可达节点必须给出说明");
}

/// 规格 user story 25：节点作用域命令只落在目标节点。
#[tokio::test]
async fn forwards_node_scoped_requests_to_that_node_only() {
    let harness = Harness::start().await;
    let laptop = AgentHandle::start().await;
    let server = AgentHandle::start().await;
    harness.register_node("laptop", &laptop.url).await;
    harness.register_node("server", &server.url).await;

    let path = laptop.directory.path().to_str().expect("临时路径是 UTF-8");
    let response = reqwest::Client::new()
        .post(harness.url("/api/n/laptop/workspaces"))
        .json(&serde_json::json!({ "path": path }))
        .send()
        .await
        .expect("经 hub 注册 workspace");
    assert_eq!(response.status(), StatusCode::CREATED);

    let laptop = harness.snapshot("laptop").await;
    assert_eq!(
        laptop.bootstrap.expect("带 bootstrap").workspaces.len(),
        1,
        "请求应落在 laptop"
    );
    let server = harness.snapshot("server").await;
    assert_eq!(
        server.bootstrap.expect("带 bootstrap").workspaces.len(),
        0,
        "server 不应收到该请求"
    );
}

/// 规格 user story 27：未知节点名返回显式错误。
#[tokio::test]
async fn rejects_unknown_node() {
    let harness = Harness::start().await;

    let response = reqwest::get(harness.url("/api/n/missing/bootstrap"))
        .await
        .expect("请求未知节点");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body: hub::HubErrorResponse = response.json().await.expect("解析错误响应");
    assert_eq!(body.code, hub::HubErrorCode::UnknownNode);
}

/// 规格 user story 28：不可达节点的命令返回显式错误。
#[tokio::test]
async fn rejects_commands_to_unreachable_node() {
    let harness = Harness::start().await;
    harness.register_node("offline", "http://127.0.0.1:9").await;

    let response = reqwest::get(harness.url("/api/n/offline/bootstrap"))
        .await
        .expect("请求不可达节点");
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body: hub::HubErrorResponse = response.json().await.expect("解析错误响应");
    assert_eq!(body.code, hub::HubErrorCode::NodeUnavailable);
}

/// 规格 user story 15：协议版本不匹配的节点状态为 incompatible，命令被拒绝。
#[tokio::test]
async fn marks_incompatible_agent_and_rejects_its_commands() {
    let harness = Harness::start().await;
    let fake = start_fake_agent(hub::REQUIRED_AGENT_PROTOCOL_VERSION + 1).await;
    harness.register_node("fake", &fake).await;

    let snapshot = harness.snapshot("fake").await;
    assert_eq!(snapshot.status, hub::NodeStatus::Incompatible);
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
