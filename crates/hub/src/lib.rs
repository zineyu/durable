use std::fmt;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::{FromRequest, Path as UrlPath, Request, State};
use axum::http::{HeaderValue, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use durable::{NodeName, Store};
use rust_embed::Embed;
use serde::de::DeserializeOwned;
use ts_rs::TS;

mod error;
mod protocol;

pub use error::ApiError;
pub use protocol::{
    HUB_PROTOCOL_VERSION, HubBootstrap, HubErrorCode, HubErrorResponse, HubHealth, HubStatus,
    NodeSnapshot, NodeStatus, RegisterNodeRequest,
};

pub const DEFAULT_LISTEN: &str = "127.0.0.1:8080";

#[derive(Embed)]
#[folder = "$CARGO_MANIFEST_DIR/../../client/dist"]
struct Assets;

#[derive(Clone)]
struct HubState {
    store: Arc<Store>,
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

pub fn router(store: Arc<Store>) -> Router {
    let state = HubState { store };
    Router::new()
        .route("/api/health", get(health))
        .route("/api/bootstrap", get(bootstrap))
        .route("/api/nodes", post(register_node))
        .route("/api/nodes/{name}", delete(remove_node))
        .fallback(static_asset)
        .with_state(state)
}

pub async fn serve(listener: tokio::net::TcpListener, store: Arc<Store>) -> std::io::Result<()> {
    axum::serve(listener, router(store)).await
}

pub fn export_types(directory: &Path) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(directory)?;
    let config = ts_rs::Config::new().with_out_dir(directory);
    HubStatus::export_all(&config)?;
    HubHealth::export_all(&config)?;
    NodeStatus::export_all(&config)?;
    NodeSnapshot::export_all(&config)?;
    HubBootstrap::export_all(&config)?;
    RegisterNodeRequest::export_all(&config)?;
    HubErrorCode::export_all(&config)?;
    HubErrorResponse::export_all(&config)?;
    NodeName::export_all(&config)?;
    Ok(())
}

async fn health() -> Json<HubHealth> {
    Json(HubHealth {
        status: HubStatus::Ok,
        version: env!("CARGO_PKG_VERSION").to_owned(),
        protocol_version: HUB_PROTOCOL_VERSION,
    })
}

async fn bootstrap(State(state): State<HubState>) -> Result<Json<HubBootstrap>, ApiError> {
    let nodes = state.store.nodes().await?;
    Ok(Json(HubBootstrap {
        nodes: nodes
            .into_iter()
            .map(|node| NodeSnapshot::unattached(node, None))
            .collect(),
    }))
}

async fn register_node(
    State(state): State<HubState>,
    JsonBody(request): JsonBody<RegisterNodeRequest>,
) -> Result<(StatusCode, Json<NodeSnapshot>), ApiError> {
    let name = NodeName::parse(&request.name)?;
    let url = request.url.trim();
    if url.is_empty() {
        return Err(ApiError::invalid_request(
            StatusCode::UNPROCESSABLE_ENTITY,
            "节点 URL 不能为空".to_owned(),
        ));
    }
    let node = state.store.register_node(name, url.to_owned()).await?;
    Ok((
        StatusCode::CREATED,
        Json(NodeSnapshot::unattached(node, None)),
    ))
}

async fn remove_node(
    State(state): State<HubState>,
    UrlPath(name): UrlPath<String>,
) -> Result<StatusCode, ApiError> {
    let name = NodeName::parse(&name)?;
    state.store.remove_node(&name).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// 请求体；反序列化失败时返回协议错误信封而非纯文本。
struct JsonBody<T>(T);

impl<S, T> FromRequest<S> for JsonBody<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        let Json(value) =
            Json::<T>::from_request(request, state)
                .await
                .map_err(|rejection: JsonRejection| {
                    ApiError::invalid_request(rejection.status(), rejection.body_text())
                })?;
        Ok(Self(value))
    }
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
