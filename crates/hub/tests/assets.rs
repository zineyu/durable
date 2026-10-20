mod support;

use support::Harness;

/// 规格「进程与部署」：hub 托管内嵌的客户端资源。
#[tokio::test]
async fn serves_embedded_index() {
    let harness = Harness::start().await;

    let response = reqwest::get(harness.url("/")).await.expect("请求首页");
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

    let missing = reqwest::get(harness.url("/missing.js"))
        .await
        .expect("请求未知路径");
    assert_eq!(missing.status(), reqwest::StatusCode::NOT_FOUND);
}
