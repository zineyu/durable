#![expect(
    clippy::expect_used,
    reason = "测试脚手架用 expect 直接报告夹具失败：夹具失败必须立刻暴露。"
)]

mod support;

use reqwest::StatusCode;
use support::{AgentHandle, Harness};

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
    assert_eq!(error_code(old).await, hub::HubErrorCode::UnknownNode);

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
    assert_eq!(error_code(unknown).await, hub::HubErrorCode::UnknownNode);

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
    let mut stream = support::EventStream::connect(&harness, cursor).await;

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
    assert_eq!(error_code(response).await, hub::HubErrorCode::UnknownNode);
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
