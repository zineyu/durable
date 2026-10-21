//! hub 二进制库：路由、节点表命令、节点作用域转发、静态资源与类型导出入口。
//! 总线与上游订阅在 `bus`，合并事件流在 `sse`。

use std::collections::HashMap;
use std::fmt;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::extract::rejection::JsonRejection;
use axum::extract::{FromRequest, Path as UrlPath, Request, State};
use axum::http::{HeaderValue, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, delete, get, post};
use axum::{Json, Router};
use durable::{NodeMetadata, NodeName, Store};
use futures_util::future::join_all;
use futures_util::{Stream, StreamExt, TryStreamExt};
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
    RenameNodeRequest,
};

pub const DEFAULT_LISTEN: &str = "127.0.0.1:8080";
/// 未指定 `--database` 时使用的数据库文件名。
pub const DEFAULT_DATABASE_FILE: &str = "hub.sqlite";
/// 日志文件的前缀。
pub const LOG_FILE_PREFIX: &str = "hub";
const DEFAULT_BROADCAST_CAPACITY: usize = 4096;
const DEFAULT_REPLAY_WINDOW: usize = 4096;
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
/// 上游事件流的空闲超时。等于 3 个上游 keep-alive 间隔（15 秒）。
const DEFAULT_STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(45);
const DEFAULT_BACKOFF_BASE: Duration = Duration::from_secs(1);

#[derive(Embed)]
#[folder = "$CARGO_MANIFEST_DIR/../../client/dist"]
struct Assets;

/// hub 的可调参数。测试用较小的容量以覆盖落后与窗口外游标，用较短的时长以覆盖超时与退避。
#[derive(Debug, Clone, Copy)]
pub struct HubOptions {
    pub broadcast_capacity: usize,
    pub replay_window: usize,
    /// 连接上游的超时，也是 health 与 bootstrap 探测的总时限。
    pub connect_timeout: Duration,
    /// 命令转发的总超时，从请求开始算到响应读完。
    pub command_timeout: Duration,
    /// 转发事件流的逐块空闲超时。
    pub stream_idle_timeout: Duration,
    /// 上游重连退避的底数。上限是它的 8 倍。
    pub backoff_base: Duration,
}

impl Default for HubOptions {
    fn default() -> Self {
        Self {
            broadcast_capacity: DEFAULT_BROADCAST_CAPACITY,
            replay_window: DEFAULT_REPLAY_WINDOW,
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            command_timeout: DEFAULT_COMMAND_TIMEOUT,
            stream_idle_timeout: DEFAULT_STREAM_IDLE_TIMEOUT,
            backoff_base: DEFAULT_BACKOFF_BASE,
        }
    }
}

#[derive(Clone)]
pub struct HubState {
    pub store: Arc<Store>,
    pub bus: Arc<Bus>,
    client: reqwest::Client,
    options: HubOptions,
    watchers: Arc<Mutex<HashMap<NodeName, bus::NodeWatcher>>>,
}

impl HubState {
    /// 打开状态并为节点表中已有的节点启动上游订阅。
    ///
    /// # Errors
    ///
    /// - [`std::io::Error`]：HTTP client 无法建立、节点表读取失败，或 watcher 表的状态锁已被毒化。
    pub async fn start(store: Arc<Store>, options: HubOptions) -> std::io::Result<Self> {
        let bus = Arc::new(Bus::new(options.broadcast_capacity, options.replay_window));
        let client = reqwest::Client::builder()
            .connect_timeout(options.connect_timeout)
            .build()
            .map_err(std::io::Error::other)?;
        let state = Self {
            store,
            bus,
            client,
            options,
            watchers: Arc::new(Mutex::new(HashMap::new())),
        };
        let nodes = match state.store.nodes().await {
            Ok(nodes) => nodes,
            Err(error) => {
                return Err(std::io::Error::other(error.to_string()));
            }
        };
        tracing::info!(nodes = nodes.len(), "恢复节点订阅");
        for node in nodes {
            state.watch(node).map_err(std::io::Error::other)?;
        }
        Ok(state)
    }

    fn watch(&self, node: NodeMetadata) -> Result<(), ApiError> {
        let watcher = bus::NodeWatcher::start(
            Arc::clone(&self.bus),
            node.clone(),
            self.client.clone(),
            self.options,
        );
        self.watchers()?.insert(node.name, watcher);
        Ok(())
    }

    fn unwatch(&self, name: &NodeName) -> Result<(), ApiError> {
        self.watchers()?.remove(name);
        Ok(())
    }

    /// 取 watcher 表的锁。毒化之后共享状态不再可信，只能作为内部错误上报。
    fn watchers(&self) -> Result<MutexGuard<'_, HashMap<NodeName, bus::NodeWatcher>>, ApiError> {
        self.watchers
            .lock()
            .map_err(|_| ApiError::internal("watchers 状态锁已毒化".to_owned()))
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

/// 解析监听地址，只接受回环地址。
///
/// # Errors
///
/// - [`ListenAddressError::Invalid`]：字符串不是合法的 `SocketAddr`。
/// - [`ListenAddressError::NotLoopback`]：地址不是回环地址。
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
        .route("/api/nodes/{name}", delete(remove_node).patch(rename_node))
        .route("/api/n/{node}", any(forward_node))
        .route("/api/n/{node}/{*rest}", any(forward_node))
        .fallback(static_asset)
        .with_state(state)
}

/// 用默认参数启动 hub 并开始监听。
///
/// # Errors
///
/// - [`std::io::Error`]：状态初始化失败，或监听或服务过程出错。
pub async fn serve(listener: tokio::net::TcpListener, store: Arc<Store>) -> std::io::Result<()> {
    serve_with(listener, store, HubOptions::default()).await
}

/// 用给定参数启动 hub 并开始监听。
///
/// # Errors
///
/// - [`std::io::Error`]：状态初始化失败，或监听或服务过程出错。
pub async fn serve_with(
    listener: tokio::net::TcpListener,
    store: Arc<Store>,
    options: HubOptions,
) -> std::io::Result<()> {
    let state = HubState::start(store, options).await?;
    axum::serve(listener, router(state)).await
}

/// 把协议类型导出为 TypeScript 定义。
///
/// # Errors
///
/// - [`std::io::Error`]：输出目录无法建立。
/// - [`ts_rs::ExportError`]：某个类型无法导出。
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
    RenameNodeRequest::export_all(&config)?;
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
    let max_hub_seq = state.bus.max_seq()?;
    let nodes = state.store.nodes().await?;
    let snapshots = join_all(nodes.into_iter().map(|node| snapshot_of(&state, node)))
        .await
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(HubBootstrap {
        nodes: snapshots,
        max_hub_seq,
    }))
}

/// 单个节点的快照。总线认为不可达的节点不再探测，直接回它的说明。
///
/// 探测失败只体现在这次快照里，不回写总线视图：节点状态的唯一写入者是 watcher
/// （ADR 0028）。「这次能否取到元数据」与「上游事件流是否通」是两个观测，允许短暂不一致。
async fn snapshot_of(state: &HubState, node: NodeMetadata) -> Result<NodeSnapshot, ApiError> {
    let view = state.bus.view(&node.name)?;
    Ok(match view.status {
        NodeStatus::Ok => {
            match node_probe::fetch_bootstrap(&state.client, &node, state.options.connect_timeout)
                .await
            {
                Ok(bootstrap) => NodeSnapshot::ok(node, bootstrap),
                Err(detail) => NodeSnapshot::failed(node, NodeStatus::Unreachable, detail),
            }
        }
        status => NodeSnapshot::failed(
            node,
            status,
            view.detail.unwrap_or_else(|| "节点不可用".to_owned()),
        ),
    })
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
    state.bus.node_registered(node.clone())?;
    state.watch(node.clone())?;
    tracing::info!(node = %node.name, url = %node.url, "注册节点");
    Ok((
        StatusCode::CREATED,
        Json(NodeSnapshot::failed(
            node,
            NodeStatus::Unreachable,
            "正在建立上游连接".to_owned(),
        )),
    ))
}

/// 重命名节点。旧名立即失效，总线广播旧名的删除与新名的登记。
async fn rename_node(
    State(state): State<HubState>,
    UrlPath(name): UrlPath<String>,
    JsonBody(request): JsonBody<RenameNodeRequest>,
) -> Result<Json<NodeSnapshot>, ApiError> {
    let name = NodeName::parse(&name)?;
    let next = NodeName::parse(&request.name)?;
    let node = state.store.rename_node(&name, next).await?;
    tracing::info!(from = %name, to = %node.name, "重命名节点");
    if node.name != name {
        state.unwatch(&name)?;
        state.bus.node_removed(name)?;
        state.bus.node_registered(node.clone())?;
        state.watch(node.clone())?;
    }
    Ok(Json(NodeSnapshot::failed(
        node,
        NodeStatus::Unreachable,
        "正在建立上游连接".to_owned(),
    )))
}

async fn remove_node(
    State(state): State<HubState>,
    UrlPath(name): UrlPath<String>,
) -> Result<StatusCode, ApiError> {
    let name = NodeName::parse(&name)?;
    state.store.remove_node(&name).await?;
    tracing::info!(node = %name, "移除节点");
    state.unwatch(&name)?;
    state.bus.node_removed(name)?;
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
    let view = state.bus.view(&name)?;
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
    for (name, value) in &headers {
        if is_hop_by_hop(name.as_str()) || name == header::HOST {
            continue;
        }
        upstream = upstream.header(name, value);
    }

    let started = tokio::time::Instant::now();
    let response =
        match tokio::time::timeout(state.options.command_timeout, upstream.body(body).send()).await
        {
            Ok(Ok(response)) => response,
            Ok(Err(error)) => {
                tracing::debug!(node = %name, error = %error, "转发请求失败");
                return Err(ApiError::node_unavailable(format!("连接节点失败: {error}")));
            }
            Err(_) => {
                return Err(ApiError::node_unavailable(
                    "命令超时: 节点未在时限内响应".to_owned(),
                ));
            }
        };

    let status = response.status();
    let response_headers = response.headers().clone();

    let mut builder = Response::builder().status(status);
    for (name, value) in &response_headers {
        if is_hop_by_hop(name.as_str()) {
            continue;
        }
        builder = builder.header(name, value);
    }

    let body = if is_event_stream(&response_headers) {
        Body::from_stream(idle_body(
            response.bytes_stream(),
            state.options.stream_idle_timeout,
            &name,
        ))
    } else {
        let deadline = started + state.options.command_timeout;
        match tokio::time::timeout_at(deadline, response.bytes()).await {
            Ok(Ok(bytes)) => Body::from(bytes),
            Ok(Err(error)) => {
                return Err(ApiError::node_unavailable(format!(
                    "读取节点响应失败: {error}"
                )));
            }
            Err(_) => {
                return Err(ApiError::node_unavailable(
                    "命令超时: 节点未在时限内完成响应".to_owned(),
                ));
            }
        }
    };

    builder
        .body(body)
        .map_err(|error| ApiError::node_unavailable(format!("构造转发响应失败: {error}")))
}

/// 响应是否事件流。hub 据此选空闲超时而不是总超时，因此不需要认识 agent 的端点清单。
fn is_event_stream(headers: &axum::http::HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("text/event-stream"))
}

/// 给转发的事件流套上逐块空闲超时。超时即结束该流，由客户端重连后恢复。
fn idle_body(
    stream: impl Stream<Item = reqwest::Result<Bytes>> + Send + 'static,
    idle: Duration,
    node: &NodeName,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static {
    let node = node.clone();
    async_stream::stream! {
        let mut stream = Box::pin(stream);
        loop {
            match tokio::time::timeout(idle, stream.next()).await {
                Ok(Some(Ok(chunk))) => yield Ok(chunk),
                Ok(Some(Err(error))) => {
                    yield Err(std::io::Error::other(error));
                    return;
                }
                Ok(None) => return,
                Err(_) => {
                    tracing::debug!(node = %node, idle_ms = idle.as_millis(), "转发的事件流空闲超时");
                    return;
                }
            }
        }
    }
}

fn split_node_path(path: &str) -> Result<(&str, &str), ApiError> {
    let remainder = path
        .strip_prefix("/api/n/")
        .ok_or_else(|| ApiError::unknown_node(format!("不是节点作用域路径: {path}")))?;
    match remainder.find('/') {
        Some(index) => Ok(remainder.split_at(index)),
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
