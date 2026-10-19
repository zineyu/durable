use std::net::{Ipv4Addr, SocketAddr};

#[tokio::test]
async fn health_endpoint_reports_ok() {
    let directory = tempfile::tempdir().expect("创建临时目录");
    let store = agent::Store::open(&directory.path().join("agent.sqlite")).expect("打开 SQLite");
    let listener = tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
        .await
        .expect("绑定回环端口");
    let address = listener.local_addr().expect("读取本地地址");
    tokio::spawn(async move {
        agent::serve(
            listener,
            agent::AgentState::new(store, "test-model".to_owned()),
        )
        .await
        .expect("agent 服务运行");
    });

    let response = reqwest::get(format!("http://{address}/api/health"))
        .await
        .expect("请求 health 端点");
    assert_eq!(response.status(), reqwest::StatusCode::OK);

    let body: agent::HealthResponse = response.json().await.expect("解析 health 响应");
    assert_eq!(body.status, agent::HealthStatus::Ok);
    assert_eq!(body.version, env!("CARGO_PKG_VERSION"));
}
