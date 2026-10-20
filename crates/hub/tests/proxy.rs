use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use tokio::sync::Notify;

async fn start_agent() -> (tempfile::TempDir, SocketAddr) {
    let directory = tempfile::tempdir().expect("创建临时目录");
    let store = agent::Store::open(&directory.path().join("agent.sqlite"))
        .await
        .expect("打开 SQLite");
    let model = agent::ModelConfig {
        base_url: "http://127.0.0.1:9/v1".to_owned(),
        api_key: "test-key".to_owned(),
        model_name: "test-model".to_owned(),
    };
    let provider = Arc::new(agent::OpenAiProvider::new(&model));
    let state = agent::AgentState::start(
        store,
        "test-model".to_owned(),
        provider,
        Arc::new(agent::NoTools),
        agent::EngineConfig::default(),
    )
    .await
    .expect("启动 agent 状态");
    let listener = tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
        .await
        .expect("绑定 agent 端口");
    let address = listener.local_addr().expect("读取 agent 地址");
    tokio::spawn(async move {
        agent::serve(listener, state).await.expect("agent 服务运行");
    });
    (directory, address)
}

/// 分段发送事件流的上游服务；第一段之后等到 `release` 被通知才发第二段。
async fn start_streaming_upstream(release: Arc<Notify>) -> SocketAddr {
    let router = axum::Router::new().route(
        "/api/events",
        axum::routing::get(move || {
            let release = Arc::clone(&release);
            async move {
                let head = futures_util::stream::once(async {
                    Ok::<_, std::io::Error>(axum::body::Bytes::from_static(b"data: first\n\n"))
                });
                let tail = futures_util::stream::once(async move {
                    release.notified().await;
                    Ok::<_, std::io::Error>(axum::body::Bytes::from_static(b"data: second\n\n"))
                });
                let mut response =
                    axum::response::Response::new(axum::body::Body::from_stream(head.chain(tail)));
                response.headers_mut().insert(
                    axum::http::header::CONTENT_TYPE,
                    axum::http::HeaderValue::from_static("text/event-stream"),
                );
                response
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
        .await
        .expect("绑定上游端口");
    let address = listener.local_addr().expect("读取上游地址");
    tokio::spawn(async move {
        axum::serve(listener, router).await.expect("上游服务运行");
    });
    address
}

async fn start_hub(agent_url: String) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
        .await
        .expect("绑定 hub 端口");
    let address = listener.local_addr().expect("读取 hub 地址");
    tokio::spawn(async move {
        hub::serve(listener, agent_url).await.expect("hub 服务运行");
    });
    address
}

#[tokio::test]
async fn health_is_reachable_through_hub_proxy() {
    let (_directory, agent_address) = start_agent().await;
    let hub_address = start_hub(format!("http://{agent_address}")).await;

    let response = reqwest::get(format!("http://{hub_address}/api/health"))
        .await
        .expect("经 hub 请求 health");
    assert_eq!(response.status(), reqwest::StatusCode::OK);

    let body: agent::HealthResponse = response.json().await.expect("解析 health 响应");
    assert_eq!(body.status, agent::HealthStatus::Ok);
    assert_eq!(body.version, env!("CARGO_PKG_VERSION"));
}

#[tokio::test]
async fn serves_embedded_index() {
    let (_directory, agent_address) = start_agent().await;
    let hub_address = start_hub(format!("http://{agent_address}")).await;

    let response = reqwest::get(format!("http://{hub_address}/"))
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

    let missing = reqwest::get(format!("http://{hub_address}/missing.js"))
        .await
        .expect("请求未知路径");
    assert_eq!(missing.status(), reqwest::StatusCode::NOT_FOUND);
}

/// 规格「hub 反代」：SSE 不缓冲，上游发一段就转一段。
#[tokio::test]
async fn streams_server_sent_events_without_buffering() {
    let release = Arc::new(Notify::new());
    let upstream = start_streaming_upstream(Arc::clone(&release)).await;
    let hub_address = start_hub(format!("http://{upstream}")).await;

    let response = tokio::time::timeout(
        Duration::from_secs(5),
        reqwest::get(format!("http://{hub_address}/api/events")),
    )
    .await
    .expect("代理返回响应头")
    .expect("经 hub 订阅事件流");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("text/event-stream")
    );
    let mut stream = response.bytes_stream();

    let first = tokio::time::timeout(Duration::from_secs(5), stream.next())
        .await
        .expect("首条事件不得等待后续事件")
        .expect("事件流未结束")
        .expect("读取首条事件");
    assert_eq!(first.as_ref(), b"data: first\n\n");

    release.notify_one();
    let second = tokio::time::timeout(Duration::from_secs(5), stream.next())
        .await
        .expect("等待第二条事件")
        .expect("事件流未结束")
        .expect("读取第二条事件");
    assert_eq!(second.as_ref(), b"data: second\n\n");

    let end = tokio::time::timeout(Duration::from_secs(5), stream.next())
        .await
        .expect("上游结束后关闭流");
    assert!(end.is_none(), "上游结束后不再有事件");
}
