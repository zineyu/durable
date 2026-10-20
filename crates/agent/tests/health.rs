mod support;

use support::Harness;

#[tokio::test]
async fn health_endpoint_reports_ok() {
    let harness = Harness::start().await;

    let response = reqwest::get(harness.url("/api/health"))
        .await
        .expect("请求 health 端点");
    assert_eq!(response.status(), reqwest::StatusCode::OK);

    let body: agent::HealthResponse = response.json().await.expect("解析 health 响应");
    assert_eq!(body.status, agent::HealthStatus::Ok);
    assert_eq!(body.version, env!("CARGO_PKG_VERSION"));
    assert_eq!(body.protocol_version, agent::PROTOCOL_VERSION);
}
