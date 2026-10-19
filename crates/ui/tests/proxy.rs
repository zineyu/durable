use std::net::{Ipv4Addr, SocketAddr};

async fn start_agent() -> (tempfile::TempDir, SocketAddr) {
    let directory = tempfile::tempdir().expect("创建临时目录");
    let store = agent::Store::open(&directory.path().join("agent.sqlite")).expect("打开 SQLite");
    let listener = tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
        .await
        .expect("绑定 agent 端口");
    let address = listener.local_addr().expect("读取 agent 地址");
    tokio::spawn(async move {
        agent::serve(
            listener,
            agent::AgentState::new(store, "test-model".to_owned()),
        )
        .await
        .expect("agent 服务运行");
    });
    (directory, address)
}

async fn start_ui(agent_url: String) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
        .await
        .expect("绑定 ui 端口");
    let address = listener.local_addr().expect("读取 ui 地址");
    tokio::spawn(async move {
        ui::serve(listener, agent_url).await.expect("ui 服务运行");
    });
    address
}

#[tokio::test]
async fn health_is_reachable_through_ui_proxy() {
    let (_directory, agent_address) = start_agent().await;
    let ui_address = start_ui(format!("http://{agent_address}")).await;

    let response = reqwest::get(format!("http://{ui_address}/api/health"))
        .await
        .expect("经 ui 请求 health");
    assert_eq!(response.status(), reqwest::StatusCode::OK);

    let body: agent::HealthResponse = response.json().await.expect("解析 health 响应");
    assert_eq!(body.status, agent::HealthStatus::Ok);
    assert_eq!(body.version, env!("CARGO_PKG_VERSION"));
}

#[tokio::test]
async fn serves_embedded_index() {
    let (_directory, agent_address) = start_agent().await;
    let ui_address = start_ui(format!("http://{agent_address}")).await;

    let response = reqwest::get(format!("http://{ui_address}/"))
        .await
        .expect("请求首页");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .expect("首页带 content-type")
        .to_owned();
    assert!(
        content_type.starts_with("text/html"),
        "content-type: {content_type}"
    );

    let body = response.text().await.expect("读取首页");
    assert!(body.contains("<div id=\"root\">"), "首页缺少根节点");

    let missing = reqwest::get(format!("http://{ui_address}/missing.js"))
        .await
        .expect("请求未知路径");
    assert_eq!(missing.status(), reqwest::StatusCode::NOT_FOUND);
}
