use std::fmt;
use std::net::SocketAddr;

use axum::Router;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderValue, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use futures_util::TryStreamExt;
use rust_embed::Embed;

pub const DEFAULT_LISTEN: &str = "127.0.0.1:8080";
pub const DEFAULT_AGENT_URL: &str = "http://127.0.0.1:8081";

#[derive(Embed)]
#[folder = "$CARGO_MANIFEST_DIR/../../client/dist"]
struct Assets;

#[derive(Clone)]
struct HubState {
    agent_url: String,
    client: reqwest::Client,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListenAddressError {
    Invalid(String),
    NotLoopback(String),
}

impl fmt::Display for ListenAddressError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(raw) => write!(formatter, "无法解析监听地址: {raw}"),
            Self::NotLoopback(raw) => write!(formatter, "监听地址必须是回环地址: {raw}"),
        }
    }
}

impl std::error::Error for ListenAddressError {}

pub fn parse_listen_address(raw: &str) -> Result<SocketAddr, ListenAddressError> {
    let address: SocketAddr = raw
        .parse()
        .map_err(|_| ListenAddressError::Invalid(raw.to_owned()))?;
    if !address.ip().is_loopback() {
        return Err(ListenAddressError::NotLoopback(raw.to_owned()));
    }
    Ok(address)
}

pub fn router(agent_url: String) -> Router {
    let state = HubState {
        agent_url: agent_url.trim_end_matches('/').to_owned(),
        client: reqwest::Client::new(),
    };
    Router::new()
        .route("/api", any(proxy))
        .route("/api/{*path}", any(proxy))
        .fallback(static_asset)
        .with_state(state)
}

pub async fn serve(listener: tokio::net::TcpListener, agent_url: String) -> std::io::Result<()> {
    axum::serve(listener, router(agent_url)).await
}

async fn proxy(State(state): State<HubState>, request: Request) -> Response {
    let method = request.method().clone();
    let uri = request.uri().clone();
    let headers = request.headers().clone();
    let target = format!(
        "{}{}{}",
        state.agent_url,
        uri.path(),
        uri.query()
            .map(|query| format!("?{query}"))
            .unwrap_or_default()
    );

    let body = reqwest::Body::wrap_stream(
        request
            .into_body()
            .into_data_stream()
            .map_err(std::io::Error::other),
    );

    let mut upstream = state.client.request(method, target);
    for (name, value) in headers.iter() {
        if is_hop_by_hop(name.as_str()) || name == header::HOST {
            continue;
        }
        upstream = upstream.header(name, value);
    }

    let response = match upstream.body(body).send().await {
        Ok(response) => response,
        Err(error) => {
            return (StatusCode::BAD_GATEWAY, format!("连接 agent 失败: {error}")).into_response();
        }
    };

    let status = response.status();
    let response_headers = response.headers().clone();
    let stream = response.bytes_stream().map_err(std::io::Error::other);

    let mut builder = Response::builder().status(status);
    for (name, value) in response_headers.iter() {
        if is_hop_by_hop(name.as_str()) {
            continue;
        }
        builder = builder.header(name, value);
    }

    builder
        .body(Body::from_stream(stream))
        .unwrap_or_else(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("构造反代响应失败: {error}"),
            )
                .into_response()
        })
}

async fn static_asset(uri: Uri) -> Response {
    let requested = uri.path().trim_start_matches('/');
    let path = if requested.is_empty() {
        "index.html"
    } else {
        requested
    };

    match Assets::get(path) {
        Some(asset) => asset_response(path, asset.data.into_owned()),
        None => (StatusCode::NOT_FOUND, "资源不存在").into_response(),
    }
}

fn asset_response(path: &str, data: Vec<u8>) -> Response {
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let content_type = HeaderValue::from_str(mime.as_ref())
        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));
    ([(header::CONTENT_TYPE, content_type)], data).into_response()
}

fn is_hop_by_hop(name: &str) -> bool {
    matches!(
        name,
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}
