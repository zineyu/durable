mod support;

#[tokio::test]
async fn health_endpoint_reports_ok() {
    let directory = tempfile::tempdir().expect("创建临时目录");
    let agent =
        support::TestAgent::start(&directory.path().join("agent.sqlite"), "test-model").await;

    let response = reqwest::get(agent.url("/api/health"))
        .await
        .expect("请求 health 端点");
    assert_eq!(response.status(), reqwest::StatusCode::OK);

    let body: agent::HealthResponse = response.json().await.expect("解析 health 响应");
    assert_eq!(body.status, agent::HealthStatus::Ok);
    assert_eq!(body.version, env!("CARGO_PKG_VERSION"));
}
