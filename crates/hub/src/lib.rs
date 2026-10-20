use std::collections::HashMap;
use std::fmt;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::rejection::JsonRejection;
use axum::extract::{FromRequest, Path as UrlPath, Request, State};
use axum::http::{HeaderValue, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, delete, get, post};
use axum::{Json, Router};
use durable::{NodeMetadata, NodeName, Store};
use futures_util::TryStreamExt;
use rust_embed::Embed;
use serde::de::DeserializeOwned;
use ts_rs::TS;

mod bus;
mod error;
mod node_probe;
mod protocol;
mod sse;

pub use bus::{Bus, NodeView};
pub use error::ApiError;
pub use protocol::{
    HUB_PROTOCOL_VERSION, HubBootstrap, HubErrorCode, HubErrorResponse, HubHealth, HubStatus,
    HubStreamEvent, NodeSnapshot, NodeStatus, REQUIRED_AGENT_PROTOCOL_VERSION, RegisterNodeRequest,
};

pub const DEFAULT_LISTEN: &str = "127.0.0.1:8080";
const DEFAULT_BROADCAST_CAPACITY: usize = 4096;
const DEFAULT_REPLAY_WINDOW: usize = 4096;

#[derive(Embed)]
#[folder = "$CARGO_MANIFEST_DIR/../../client/dist"]
struct Assets;

/// hub 的可调参数。测试用较小的容量以覆盖落后与窗口外游标。
#[derive(Debug, Clone, Copy)]
pub struct HubOptions {
    pub broadcast_capacity: usize,
    pub replay_window: usize,
}

impl Default for HubOptions {
    fn default() -> Self {
        Self {
            broadcast_capacity: DEFAULT_BROADCAST_CAPACITY,
            replay_window: DEFAULT_REPLAY_WINDOW,
        }
    }
}

#[derive(Clone)]
pub struct HubState {
    pub store: Arc<Store>,
    pub bus: Arc<Bus>,
    client: reqwest::Client,
    watchers: Arc<Mutex<HashMap<NodeName, bus::NodeWatcher>>>,
}

impl HubState {
    /// 打开状态并为节点表中已有的节点启动上游订阅。
    pub async fn start(store: Arc<Store>, options: HubOptions) -> std::io::Result<Self> {
        let bus = Arc::new(Bus::new(options.broadcast_capacity, options.replay_window));
        let state = Self {
            store,
            bus,
            client: reqwest::Client::new(),
            watchers: Arc::new(Mutex::new(HashMap::new())),
        };
        let nodes = match state.store.nodes().await {
            Ok(nodes) => nodes,
            Err(error) => {
                return Err(std::io::Error::other(error.to_string()));
            }
        };
        for node in nodes {
            state.watch(node);
        }
        Ok(state)
    }

    fn watch(&self, node: NodeMetadata) {
        let watcher = bus::NodeWatcher::start(Arc::clone(&self.bus), node.clone());
        self.watchers
            .lock()
            .expect("watchers 锁")
            .insert(node.name, watcher);
    }

    fn unwatch(&self, name: &NodeName) {
        self.watchers.lock().expect("watchers 锁").remove(name);
    }
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

pub fn router(state: HubState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/bootstrap", get(bootstrap))
        .route("/api/events", get(sse::events))
        .route("/api/nodes", post(register_node))
        .route("/api/nodes/{name}", delete(remove_node))
        .route("/api/n/{node}", any(forward_node))
        .route("/api/n/{node}/{*rest}", any(forward_node))
        .fallback(static_asset)
        .with_state(state)
}

pub async fn serve(listener: tokio::net::TcpListener, store: Arc<Store>) -> std::io::Result<()> {
    serve_with(listener, store, HubOptions::default()).await
}

pub async fn serve_with(
    listener: tokio::net::TcpListener,
    store: Arc<Store>,
    options: HubOptions,
) -> std::io::Result<()> {
    let state = HubState::start(store, options).await?;
    axum::serve(listener, router(state)).await
}

pub fn export_types(directory: &Path) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(directory)?;
    let config = ts_rs::Config::new().with_out_dir(directory);
    HubStatus::export_all(&config)?;
    HubHealth::export_all(&config)?;
    NodeStatus::export_all(&config)?;
    NodeSnapshot::export_all(&config)?;
    HubBootstrap::export_all(&config)?;
    HubStreamEvent::export_all(&config)?;
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
    let mut snapshots = Vec::with_capacity(nodes.len());
    for node in nodes {
        let view = state.bus.view(&node.name);
        let snapshot = match view.status {
            NodeStatus::Ok => match node_probe::fetch_bootstrap(&state.client, &node).await {
                Ok(bootstrap) => NodeSnapshot::ok(node, bootstrap),
                Err(detail) => NodeSnapshot::failed(node, NodeStatus::Unreachable, detail),
            },
            status => NodeSnapshot::failed(
                node,
                status,
                view.detail.unwrap_or_else(|| "节点不可用".to_owned()),
            ),
        };
        snapshots.push(snapshot);
    }
    Ok(Json(HubBootstrap {
        nodes: snapshots,
        max_hub_seq: state.bus.max_seq(),
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
    state.bus.node_registered(node.clone());
    state.watch(node.clone());
    Ok((
        StatusCode::CREATED,
        Json(NodeSnapshot::failed(
            node,
            NodeStatus::Unreachable,
            "正在建立上游连接".to_owned(),
        )),
    ))
}

async fn remove_node(
    State(state): State<HubState>,
    UrlPath(name): UrlPath<String>,
) -> Result<StatusCode, ApiError> {
    let name = NodeName::parse(&name)?;
    state.store.remove_node(&name).await?;
    state.unwatch(&name);
    state.bus.node_removed(name);
    Ok(StatusCode::NO_CONTENT)
}

/// 节点作用域请求：剥掉 `/api/n/<node>` 前缀后原样转发，含 SSE 流式响应。
async fn forward_node(
    State(state): State<HubState>,
    request: Request,
) -> Result<Response, ApiError> {
    let uri = request.uri().clone();
    let (raw_name, rest) = split_node_path(uri.path())?;
    let name = NodeName::parse(raw_name)?;
    let node = lookup(&state, &name).await?;
    let view = state.bus.view(&name);
    if view.status != NodeStatus::Ok {
        return Err(ApiError::node_unavailable(
            view.detail.unwrap_or_else(|| format!("节点不可用: {name}")),
        ));
    }

    let base = node_probe::base_url(&node);
    let target = format!("{base}/api{rest}{}", query_suffix(uri.query()));

    let method = request.method().clone();
    let headers = request.headers().clone();
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

    let response = upstream
        .body(body)
        .send()
        .await
        .map_err(|error| ApiError::node_unavailable(format!("连接节点失败: {error}")))?;

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
        .map_err(|error| ApiError::node_unavailable(format!("构造转发响应失败: {error}")))
}

fn split_node_path(path: &str) -> Result<(&str, &str), ApiError> {
    let remainder = path
        .strip_prefix("/api/n/")
        .ok_or_else(|| ApiError::unknown_node(format!("不是节点作用域路径: {path}")))?;
    match remainder.find('/') {
        Some(index) => Ok((&remainder[..index], &remainder[index..])),
        None => Ok((remainder, "")),
    }
}

async fn lookup(state: &HubState, name: &NodeName) -> Result<NodeMetadata, ApiError> {
    state
        .store
        .node(name)
        .await?
        .ok_or_else(|| ApiError::unknown_node(format!("节点不存在: {name}")))
}

fn query_suffix(query: Option<&str>) -> String {
    query.map(|query| format!("?{query}")).unwrap_or_default()
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
