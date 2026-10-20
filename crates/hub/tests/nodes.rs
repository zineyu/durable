mod support;

use reqwest::StatusCode;
use support::Harness;

async fn error_code(response: reqwest::Response) -> hub::HubErrorCode {
    let body: hub::HubErrorResponse = response.json().await.expect("解析错误响应");
    body.code
}

/// 规格 user story 1：注册节点不需要可达，只校验名称与 URL。
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

/// 规格 user story 3：节点名重复是显式错误。
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

/// 规格 user story 4：名称必须是路径安全字符。
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

/// 规格 user story 4：URL 不能为空。
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

/// 规格 user story 8：删除只移除登记。未知节点返回显式错误。
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

/// 规格 user story 10：重启 hub 后节点表仍在。
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
