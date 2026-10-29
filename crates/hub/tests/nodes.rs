#![expect(
    clippy::expect_used,
    reason = "测试脚手架用 expect 直接报告夹具与断言失败：夹具失败必须立刻暴露。"
)]

mod support;

use reqwest::StatusCode;
use support::fixtures::scripted::{EventsMode, ScriptedNode};
use support::harness::{AgentHandle, Harness};

async fn error_code(response: reqwest::Response) -> hub::HubErrorCode {
    let body: hub::HubErrorResponse = response.json().await.expect("解析错误响应");
    body.code
}

/// 规格 user story H-1：注册节点不需要可达，只校验名称与 URL。
#[tokio::test]
async fn registers_offline_node_and_lists_it_in_bootstrap() {
    let harness = Harness::start().await;

    let response = harness
        .register_node("laptop", "http://127.0.0.1:18081")
        .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let node: hub::NodeSnapshot = response.json().await.expect("解析节点");
    assert_eq!(node.name.as_str(), "laptop");
    assert_eq!(node.url, "http://127.0.0.1:18081");
    assert_eq!(node.status, hub::NodeStatus::Unreachable);

    let bootstrap = harness.bootstrap().await;
    assert_eq!(bootstrap.nodes.len(), 1);
    assert_eq!(bootstrap.nodes[0].name.as_str(), "laptop");
    assert_eq!(bootstrap.nodes[0].status, hub::NodeStatus::Unreachable);
}

/// 规格 user story H-3：节点名重复是显式错误。
#[tokio::test]
async fn rejects_duplicate_node_name() {
    let harness = Harness::start().await;
    harness
        .register_node("laptop", "http://127.0.0.1:18081")
        .await;

    let response = harness
        .register_node("laptop", "http://127.0.0.1:18082")
        .await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(error_code(response).await, hub::HubErrorCode::NodeNameTaken);

    let bootstrap = harness.bootstrap().await;
    assert_eq!(bootstrap.nodes.len(), 1, "重复注册不覆盖原记录");
}

/// 规格 user story H-4：名称必须是路径安全字符。
#[tokio::test]
async fn rejects_invalid_node_name() {
    let harness = Harness::start().await;

    for name in ["", "bad name", "../escape", "a/b"] {
        let response = harness.register_node(name, "http://127.0.0.1:18081").await;
        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "名称应被拒绝: {name:?}"
        );
        assert_eq!(
            error_code(response).await,
            hub::HubErrorCode::NodeNameInvalid,
            "名称应被拒绝: {name:?}"
        );
    }

    assert!(harness.bootstrap().await.nodes.is_empty());
}

/// 规格 user story H-4：URL 不能为空。
#[tokio::test]
async fn rejects_blank_node_url() {
    let harness = Harness::start().await;

    let response = harness.register_node("laptop", "   ").await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        error_code(response).await,
        hub::HubErrorCode::InvalidRequest
    );
}

/// 规格 user story H-5 与 H-6：改名保留 URL，旧名立即失效。
#[tokio::test]
async fn renames_node_and_drops_old_name() {
    let harness = Harness::start().await;
    let laptop = AgentHandle::start().await;
    harness.register_node("laptop", &laptop.url).await;
    harness.wait_for_status("laptop", hub::NodeStatus::Ok).await;

    let response = harness.rename_node("laptop", "studio").await;
    assert_eq!(response.status(), StatusCode::OK);
    let node: hub::NodeSnapshot = response.json().await.expect("解析节点");
    assert_eq!(node.name.as_str(), "studio");
    assert_eq!(node.url, laptop.url);

    let bootstrap = harness.bootstrap().await;
    assert_eq!(bootstrap.nodes.len(), 1);
    assert_eq!(bootstrap.nodes[0].name.as_str(), "studio");

    let old = reqwest::get(harness.url("/api/n/laptop/bootstrap"))
        .await
        .expect("请求旧名");
    assert_eq!(old.status(), StatusCode::NOT_FOUND);
    assert_eq!(error_code(old).await, hub::HubErrorCode::NodeNotFound);

    harness.wait_for_status("studio", hub::NodeStatus::Ok).await;
    let new = reqwest::get(harness.url("/api/n/studio/bootstrap"))
        .await
        .expect("请求新名");
    assert_eq!(new.status(), StatusCode::OK);
}

/// 规格 user story H-3 与 H-4：改名同样校验名称。
#[tokio::test]
async fn rejects_invalid_taken_or_unknown_rename() {
    let harness = Harness::start().await;
    harness
        .register_node("laptop", "http://127.0.0.1:18081")
        .await;
    harness
        .register_node("desktop", "http://127.0.0.1:18082")
        .await;

    let taken = harness.rename_node("laptop", "desktop").await;
    assert_eq!(taken.status(), StatusCode::CONFLICT);
    assert_eq!(error_code(taken).await, hub::HubErrorCode::NodeNameTaken);

    let invalid = harness.rename_node("laptop", "bad name").await;
    assert_eq!(invalid.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        error_code(invalid).await,
        hub::HubErrorCode::NodeNameInvalid
    );

    let unknown = harness.rename_node("ghost", "laptop").await;
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
    assert_eq!(error_code(unknown).await, hub::HubErrorCode::NodeNotFound);

    let names: Vec<String> = harness
        .bootstrap()
        .await
        .nodes
        .into_iter()
        .map(|node| node.name.as_str().to_owned())
        .collect();
    assert_eq!(names, ["laptop", "desktop"]);
}

/// 规格 user story H-42：改名对已打开的客户端广播旧名删除与新名登记。
#[tokio::test]
async fn broadcasts_rename_as_remove_and_register() {
    let harness = Harness::start().await;
    harness
        .register_node("laptop", "http://127.0.0.1:18081")
        .await;
    let cursor = harness.bootstrap().await.max_hub_seq;
    let mut stream = support::fixtures::EventStream::connect(&harness, cursor).await;

    let response = harness.rename_node("laptop", "studio").await;
    assert_eq!(response.status(), StatusCode::OK);

    let removed = stream
        .next_matching(|event| matches!(event, hub::HubStreamEvent::NodeRemoved { .. }))
        .await;
    let registered = stream
        .next_matching(|event| matches!(event, hub::HubStreamEvent::NodeRegistered { .. }))
        .await;
    match removed {
        hub::HubStreamEvent::NodeRemoved { hub_seq, name } => {
            assert_eq!(name.as_str(), "laptop");
            match registered {
                hub::HubStreamEvent::NodeRegistered {
                    hub_seq: next_seq,
                    node,
                } => {
                    assert!(next_seq > hub_seq, "广播序号必须递增");
                    assert_eq!(node.name.as_str(), "studio");
                }
                other => panic!("不是节点登记事件: {other:?}"),
            }
        }
        other => panic!("不是节点删除事件: {other:?}"),
    }
}

/// 规格 user story H-8：删除只移除登记。未知节点返回显式错误。
#[tokio::test]
async fn removes_node_and_rejects_unknown_node() {
    let harness = Harness::start().await;
    harness
        .register_node("laptop", "http://127.0.0.1:18081")
        .await;

    let response = harness.remove_node("laptop").await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(harness.bootstrap().await.nodes.is_empty());

    let response = harness.remove_node("laptop").await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(error_code(response).await, hub::HubErrorCode::NodeNotFound);
}

/// 规格 user story H-10：重启 hub 后节点表仍在。
#[tokio::test]
async fn restores_node_table_after_restart() {
    let harness = Harness::start().await;
    harness
        .register_node("laptop", "http://127.0.0.1:18081")
        .await;
    let directory = harness.directory;

    let restarted = Harness::start_with(directory).await;
    let bootstrap = restarted.bootstrap().await;
    assert_eq!(bootstrap.nodes.len(), 1);
    assert_eq!(bootstrap.nodes[0].name.as_str(), "laptop");
}

/// 规格 user story H-53：改 URL 保留注册时间，命令立即转发到新地址，客户端收到通知。
#[tokio::test]
async fn updates_node_url_and_forwards_to_new_address() {
    let harness = Harness::start().await;
    let first = AgentHandle::start().await;
    let second = AgentHandle::start().await;
    let mut stream = support::fixtures::EventStream::connect(&harness, 0).await;
    harness.register_node("laptop", &first.url).await;
    let registered = stream
        .next_matching(|event| matches!(event, hub::HubStreamEvent::NodeRegistered { .. }))
        .await;
    let created_at = match registered {
        hub::HubStreamEvent::NodeRegistered { node, .. } => node.created_at,
        other => panic!("不是节点登记事件: {other:?}"),
    };
    harness.wait_for_status("laptop", hub::NodeStatus::Ok).await;

    let workspace = second.directory.path().join("ws");
    std::fs::create_dir(&workspace).expect("创建 workspace");
    let workspace = workspace
        .canonicalize()
        .expect("规范化 workspace 路径")
        .to_str()
        .expect("临时路径是 UTF-8")
        .to_owned();
    let response = reqwest::Client::new()
        .post(format!("{}/api/workspaces", second.url))
        .json(&serde_json::json!({ "host": "local", "path": workspace }))
        .send()
        .await
        .expect("注册 workspace");
    assert_eq!(response.status(), StatusCode::CREATED);

    let response = harness
        .update_node("laptop", serde_json::json!({ "url": second.url }))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let updated: hub::NodeSnapshot = response.json().await.expect("解析节点");
    assert_eq!(
        updated.name.as_str(),
        "laptop",
        "节点名即身份，不随 URL 变化"
    );
    assert_eq!(updated.url, second.url);

    let event = stream
        .next_matching(|event| matches!(event, hub::HubStreamEvent::NodeUpdated { .. }))
        .await;
    match event {
        hub::HubStreamEvent::NodeUpdated { node, .. } => {
            assert_eq!(node.url, second.url);
            assert_eq!(node.created_at, created_at, "改 URL 不改变注册时间");
        }
        other => panic!("不是节点更新事件: {other:?}"),
    }

    harness.wait_for_status("laptop", hub::NodeStatus::Ok).await;
    let bootstrap: agent::BootstrapResponse = reqwest::get(harness.url("/api/n/laptop/bootstrap"))
        .await
        .expect("请求新地址的 bootstrap")
        .json()
        .await
        .expect("解析新地址的 bootstrap");
    assert_eq!(bootstrap.workspaces.len(), 1, "命令必须落在新地址");
    assert_eq!(bootstrap.workspaces[0].path.as_str(), workspace);
}

/// 规格 user story H-54 与 H-60：停用保留节点登记、状态与说明，且立即停止上游尝试。
#[tokio::test]
async fn disables_node_and_stops_upstream_attempts() {
    let harness = Harness::start_with_options(fast_options()).await;
    let node = ScriptedNode::start().await;
    node.set_events_mode(EventsMode::End);
    harness.register_node("laptop", &node.url).await;
    harness
        .wait_for_status("laptop", hub::NodeStatus::Unreachable)
        .await;

    let response = harness.set_node_enabled("laptop", false).await;
    assert_eq!(response.status(), StatusCode::OK);
    let disabled: hub::NodeSnapshot = response.json().await.expect("解析节点");
    assert!(!disabled.enabled);
    assert_eq!(disabled.status, hub::NodeStatus::Unreachable);
    assert!(
        disabled
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("已停用")),
        "状态说明必须写明已停用: {disabled:?}"
    );

    let listed = harness.snapshot("laptop").await;
    assert!(!listed.enabled, "停用的节点仍留在节点表里");
    assert_eq!(listed.status, hub::NodeStatus::Unreachable);
    assert!(
        listed
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("已停用"))
    );

    let command = reqwest::get(harness.url("/api/n/laptop/bootstrap"))
        .await
        .expect("请求停用节点的命令");
    assert_eq!(command.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        error_code(command).await,
        hub::HubErrorCode::NodeUnavailable
    );

    let reconnect = harness.reconnect_node("laptop").await;
    assert_eq!(reconnect.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        error_code(reconnect).await,
        hub::HubErrorCode::NodeUnavailable
    );

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let attempts = node.after_requests().len();
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert_eq!(
        node.after_requests().len(),
        attempts,
        "停用后不得再尝试上游连接"
    );
}

/// 规格 user story H-55：启用后立即连接，不等待退避。
#[tokio::test]
async fn enables_node_without_waiting_for_backoff() {
    let harness = Harness::start_with_options(hub::HubOptions {
        backoff_base: std::time::Duration::from_secs(5),
        ..hub::HubOptions::default()
    })
    .await;
    let node = ScriptedNode::start().await;
    node.set_events_mode(EventsMode::End);
    harness.register_node("laptop", &node.url).await;
    harness
        .wait_for_status("laptop", hub::NodeStatus::Unreachable)
        .await;
    let response = harness.set_node_enabled("laptop", false).await;
    assert_eq!(response.status(), StatusCode::OK);

    node.set_events_mode(EventsMode::Scripted);
    let started = std::time::Instant::now();
    let response = harness.set_node_enabled("laptop", true).await;
    assert_eq!(response.status(), StatusCode::OK);
    harness.wait_for_status("laptop", hub::NodeStatus::Ok).await;
    assert!(
        started.elapsed() < std::time::Duration::from_secs(1),
        "启用后必须立即连接，实际 {:?}",
        started.elapsed()
    );
}

/// 规格 user story H-56：手动重连不等待退避。
#[tokio::test]
async fn reconnects_on_demand_without_waiting_for_backoff() {
    let harness = Harness::start_with_options(hub::HubOptions {
        backoff_base: std::time::Duration::from_secs(5),
        ..hub::HubOptions::default()
    })
    .await;
    let node = ScriptedNode::start().await;
    node.set_events_mode(EventsMode::End);
    harness.register_node("laptop", &node.url).await;
    harness
        .wait_for_status("laptop", hub::NodeStatus::Unreachable)
        .await;

    node.set_events_mode(EventsMode::Scripted);
    let started = std::time::Instant::now();
    let response = harness.reconnect_node("laptop").await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    harness.wait_for_status("laptop", hub::NodeStatus::Ok).await;
    assert!(
        started.elapsed() < std::time::Duration::from_secs(1),
        "手动重连必须立即生效，实际 {:?}",
        started.elapsed()
    );
}

/// 修改节点同样校验名称与 URL。
#[tokio::test]
async fn rejects_invalid_update_fields() {
    let harness = Harness::start().await;
    harness
        .register_node("laptop", "http://127.0.0.1:18081")
        .await;
    harness
        .register_node("desktop", "http://127.0.0.1:18082")
        .await;

    let taken = harness
        .update_node("laptop", serde_json::json!({ "name": "desktop" }))
        .await;
    assert_eq!(taken.status(), StatusCode::CONFLICT);
    assert_eq!(error_code(taken).await, hub::HubErrorCode::NodeNameTaken);

    let invalid = harness
        .update_node("laptop", serde_json::json!({ "name": "bad name" }))
        .await;
    assert_eq!(invalid.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        error_code(invalid).await,
        hub::HubErrorCode::NodeNameInvalid
    );

    let blank = harness
        .update_node("laptop", serde_json::json!({ "url": "   " }))
        .await;
    assert_eq!(blank.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(error_code(blank).await, hub::HubErrorCode::InvalidRequest);

    let unknown = harness
        .update_node("ghost", serde_json::json!({ "enabled": false }))
        .await;
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
    assert_eq!(error_code(unknown).await, hub::HubErrorCode::NodeNotFound);
}

fn fast_options() -> hub::HubOptions {
    hub::HubOptions {
        backoff_base: std::time::Duration::from_millis(50),
        ..hub::HubOptions::default()
    }
}
