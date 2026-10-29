//! HTTP 路由与处理器：host、workspace、session、消息树、队列、run 与事件流。
//! 对外 DTO 在 `protocol`，错误信封在 `error`，host 的运行状态在 `hosts`。

use std::sync::Arc;

use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{FromRequest, FromRequestParts, Path, Request, State};
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use durable::{
    EventBus, HostKind, HostName, HostStatus, HostUpdate, MessageId, MessageText, QueueItem,
    QueueItemId, SessionId, SessionTitle, Store, StoreError, WorkspacePath,
};
use serde::de::DeserializeOwned;

use crate::config::EngineConfig;
use crate::engine::RunEngine;
use crate::error::ApiError;
use crate::hosts::HostRegistry;
use crate::link::ToolBox;
use crate::protocol::{
    BootstrapResponse, CreateSessionRequest, EditMessageRequest, HealthResponse, HealthStatus,
    HostSnapshot, MessageBodiesRequest, MessageBodiesResponse, PROTOCOL_VERSION,
    RegisterHostRequest, RegisterWorkspaceRequest, RemoveWorkspaceRequest, RenameSessionRequest,
    SubmitMessageRequest, SwitchActiveLeafRequest, UpdateHostRequest,
};
use crate::provider::ModelProvider;
use crate::sse;

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

/// session id 路径参数；提取失败返回协议错误信封。
struct SessionPath(SessionId);

impl<S> FromRequestParts<S> for SessionPath
where
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let Path(raw) = Path::<String>::from_request_parts(parts, state)
            .await
            .map_err(|rejection: PathRejection| {
                ApiError::invalid_request(rejection.status(), rejection.body_text())
            })?;
        Ok(Self(SessionId::parse(&raw)?))
    }
}

/// host 名路径参数；提取失败返回协议错误信封。
struct HostPath(HostName);

impl<S> FromRequestParts<S> for HostPath
where
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let Path(raw) = Path::<String>::from_request_parts(parts, state)
            .await
            .map_err(|rejection: PathRejection| {
                ApiError::invalid_request(rejection.status(), rejection.body_text())
            })?;
        Ok(Self(HostName::parse(&raw)?))
    }
}

/// `{id}/queue/{item}` 路径参数。
struct QueueItemPath {
    session: SessionId,
    item: QueueItemId,
}

/// `{id}/messages/{message}` 路径参数。
struct MessagePath {
    session: SessionId,
    message: MessageId,
}

impl<S> FromRequestParts<S> for MessagePath
where
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let Path((session, message)) = Path::<(String, i64)>::from_request_parts(parts, state)
            .await
            .map_err(|rejection: PathRejection| {
                ApiError::invalid_request(rejection.status(), rejection.body_text())
            })?;
        Ok(Self {
            session: SessionId::parse(&session)?,
            message: MessageId::parse(message)?,
        })
    }
}

impl<S> FromRequestParts<S> for QueueItemPath
where
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let Path((session, item)) = Path::<(String, i64)>::from_request_parts(parts, state)
            .await
            .map_err(|rejection: PathRejection| {
                ApiError::invalid_request(rejection.status(), rejection.body_text())
            })?;
        Ok(Self {
            session: SessionId::parse(&session)?,
            item: QueueItemId::parse(item)?,
        })
    }
}

#[derive(Clone)]
pub struct AgentState {
    store: Arc<Store>,
    hosts: Arc<HostRegistry>,
    engine: Arc<RunEngine>,
    model_name: Arc<str>,
}

impl AgentState {
    /// 组装 agent 状态并执行启动恢复。
    ///
    /// # Errors
    ///
    /// - `StoreError::Database`：读取中断的 run、读取队列或提交恢复结果时数据库操作失败。
    /// - `StoreError::Serialization`：恢复过程读出或写出的记录无法序列化。
    /// - `StoreError::CorruptedData`：存储值不符合领域约束。
    /// - `StoreError::Invariant`：存储不变量被破坏。
    pub async fn start(
        store: Arc<Store>,
        hosts: Arc<HostRegistry>,
        model_name: String,
        provider: Arc<dyn ModelProvider>,
        tools: Arc<dyn ToolBox>,
        config: EngineConfig,
    ) -> Result<Self, StoreError> {
        let engine = RunEngine::new(Arc::clone(&store), provider, tools, config);
        engine.recover().await?;
        Ok(Self {
            store,
            hosts,
            engine,
            model_name: Arc::from(model_name.as_str()),
        })
    }

    #[must_use]
    pub fn store(&self) -> &Store {
        &self.store
    }

    #[must_use]
    pub fn hosts(&self) -> &Arc<HostRegistry> {
        &self.hosts
    }

    #[must_use]
    pub fn engine(&self) -> &Arc<RunEngine> {
        &self.engine
    }

    #[must_use]
    pub fn events(&self) -> &EventBus {
        self.store.events()
    }

    #[must_use]
    pub fn model_name(&self) -> &str {
        &self.model_name
    }
}

/// 构造 agent 的 HTTP 路由。
pub fn router(state: AgentState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/bootstrap", get(bootstrap))
        .route("/api/events", get(sse::events))
        .route("/api/hosts", post(register_host))
        .route("/api/hosts/{name}", patch(update_host).delete(remove_host))
        .route("/api/hosts/{name}/reconnect", post(reconnect_host))
        .route("/api/workspaces", post(register_workspace))
        .route("/api/workspaces/remove", post(remove_workspace))
        .route("/api/sessions", post(create_session))
        .route("/api/sessions/{id}/tree", get(session_tree))
        .route("/api/sessions/{id}/message-bodies", post(message_bodies))
        .route("/api/sessions/{id}/rename", post(rename_session))
        .route("/api/sessions/{id}/delete", post(delete_session))
        .route("/api/sessions/{id}/messages", post(submit_message))
        .route(
            "/api/sessions/{id}/messages/{message}/edit",
            post(edit_user_message),
        )
        .route(
            "/api/sessions/{id}/messages/{message}/retry",
            post(retry_assistant_message),
        )
        .route("/api/sessions/{id}/active-leaf", post(switch_active_leaf))
        .route("/api/sessions/{id}/cancel", post(cancel_run))
        .route(
            "/api/sessions/{id}/queue/{item}/remove",
            post(remove_queue_item),
        )
        .with_state(state)
}

/// 在已绑定的监听器上提供 HTTP 服务，直到进程结束或出错。
///
/// # Errors
///
/// - 监听器关闭、接受连接失败或连接处理出错时返回该错误。
pub async fn serve(listener: tokio::net::TcpListener, state: AgentState) -> std::io::Result<()> {
    axum::serve(listener, router(state)).await
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: HealthStatus::Ok,
        version: env!("CARGO_PKG_VERSION").to_owned(),
        protocol_version: PROTOCOL_VERSION,
    })
}

async fn bootstrap(State(state): State<AgentState>) -> Result<Json<BootstrapResponse>, ApiError> {
    let snapshot = state.store().bootstrap().await?;
    let hosts = state.hosts().snapshots().await?;
    Ok(Json(BootstrapResponse {
        model: state.model_name().to_owned(),
        max_event_id: snapshot.max_event_id,
        hosts,
        workspaces: snapshot.workspaces,
        sessions: snapshot.sessions,
    }))
}

async fn register_host(
    State(state): State<AgentState>,
    JsonBody(request): JsonBody<RegisterHostRequest>,
) -> Result<(StatusCode, Json<HostSnapshot>), ApiError> {
    let name = HostName::parse(&request.name)?;
    let target = target_of(request.kind, request.target)?;
    let snapshot = state.hosts().register(name, request.kind, target).await?;
    Ok((StatusCode::CREATED, Json(snapshot)))
}

async fn update_host(
    State(state): State<AgentState>,
    HostPath(name): HostPath,
    JsonBody(request): JsonBody<UpdateHostRequest>,
) -> Result<Json<HostSnapshot>, ApiError> {
    let next = match request.name {
        Some(raw) => Some(HostName::parse(&raw)?),
        None => None,
    };
    let target = match request.target {
        Some(raw) => Some(non_empty_target(&raw)?),
        None => None,
    };
    let update = HostUpdate {
        name: next,
        target,
        enabled: request.enabled,
    };
    Ok(Json(state.hosts().update(&name, update).await?))
}

async fn remove_host(
    State(state): State<AgentState>,
    HostPath(name): HostPath,
) -> Result<StatusCode, ApiError> {
    state.hosts().remove(&name).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn reconnect_host(
    State(state): State<AgentState>,
    HostPath(name): HostPath,
) -> Result<Json<HostSnapshot>, ApiError> {
    state
        .hosts()
        .reconnect(&name)
        .await?
        .ok_or_else(|| ApiError::host_not_found(format!("host 不存在: {name}")))
        .map(Json)
}

/// `ssh` 种类必须给出目标；本机种类不接受目标。
fn target_of(kind: HostKind, target: Option<String>) -> Result<Option<String>, ApiError> {
    match kind {
        HostKind::Local => match target.as_deref().map(str::trim) {
            None | Some("") => Ok(None),
            Some(_) => Err(ApiError::host_invalid_target(
                "本机 host 没有 SSH 目标".to_owned(),
            )),
        },
        HostKind::Ssh => {
            let raw = target.unwrap_or_default();
            non_empty_target(&raw).map(Some)
        }
    }
}

/// SSH 目标不能为空白，也不能以 `-` 开头：`ssh` 会把它当成选项。
fn non_empty_target(raw: &str) -> Result<String, ApiError> {
    let target = raw.trim();
    if target.is_empty() {
        return Err(ApiError::host_invalid_target("SSH 目标不能为空".to_owned()));
    }
    if target.starts_with('-') {
        return Err(ApiError::host_invalid_target(
            "SSH 目标不能以 - 开头".to_owned(),
        ));
    }
    Ok(target.to_owned())
}

/// 注册 workspace：host 存在、可用，路径由 host 判定，最后才写库。
///
/// 规格 user story R-20、R-21、R-22、R-24：路径校验在 host 上完成，不可用的 host
/// 返回 `host_unavailable`，workspace 只属于一台 host。
async fn register_workspace(
    State(state): State<AgentState>,
    JsonBody(request): JsonBody<RegisterWorkspaceRequest>,
) -> Result<(StatusCode, Json<durable::WorkspaceMetadata>), ApiError> {
    let host = request.host;
    state
        .store()
        .host(&host)
        .await?
        .ok_or_else(|| ApiError::host_not_found(format!("host 不存在: {host}")))?;
    let status = state.hosts().status(&host).await;
    if !status.is_ok() {
        return Err(unavailable(&state, &host, status).await);
    }
    let requested = WorkspacePath::parse(&request.path)?;
    let path = match state
        .hosts()
        .check_workspace(&host, requested.as_str())
        .await
    {
        Some(Ok(canonical)) => WorkspacePath::parse(&canonical)?,
        Some(Err(error)) => return Err(error.into()),
        None => return Err(unavailable(&state, &host, HostStatus::Unreachable).await),
    };
    let workspace = state.store().register_workspace(&host, &path).await?;
    Ok((StatusCode::CREATED, Json(workspace)))
}

async fn unavailable(state: &AgentState, host: &HostName, status: HostStatus) -> ApiError {
    let detail = state
        .hosts()
        .detail(host)
        .await
        .unwrap_or_else(|| format!("host {host} 当前不可用"));
    tracing::debug!(host = %host, ?status, "host 不可用");
    ApiError::host_unavailable(detail)
}

async fn remove_workspace(
    State(state): State<AgentState>,
    JsonBody(request): JsonBody<RemoveWorkspaceRequest>,
) -> Result<StatusCode, ApiError> {
    let path = WorkspacePath::parse(&request.path)?;
    state.store().remove_workspace(&request.host, &path).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// 创建 session。workspace 身份是 host 与规范化绝对路径这一对，因此先让 host
/// 把请求里的拼写规范化；host 不可用时按原样查找，已登记的拼写仍然可用。
async fn create_session(
    State(state): State<AgentState>,
    JsonBody(request): JsonBody<CreateSessionRequest>,
) -> Result<(StatusCode, Json<durable::SessionMetadata>), ApiError> {
    let requested = WorkspacePath::parse(&request.workspace)?;
    let workspace = match state
        .hosts()
        .check_workspace(&request.host, requested.as_str())
        .await
    {
        Some(Ok(canonical)) => WorkspacePath::parse(&canonical)?,
        _ => requested,
    };
    let title = match request.title {
        Some(raw) => SessionTitle::parse(&raw)?,
        None => SessionTitle::default(),
    };
    let session = state
        .store()
        .create_session(&request.host, &workspace, title)
        .await?;
    Ok((StatusCode::CREATED, Json(session)))
}

async fn rename_session(
    State(state): State<AgentState>,
    SessionPath(id): SessionPath,
    JsonBody(request): JsonBody<RenameSessionRequest>,
) -> Result<Json<durable::SessionMetadata>, ApiError> {
    let title = SessionTitle::parse(&request.title)?;
    Ok(Json(state.store().rename_session(&id, &title).await?))
}

async fn delete_session(
    State(state): State<AgentState>,
    SessionPath(id): SessionPath,
) -> Result<StatusCode, ApiError> {
    state.engine().cancel(&id).await;
    state.store().delete_session(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn session_tree(
    State(state): State<AgentState>,
    SessionPath(id): SessionPath,
) -> Result<Json<durable::SessionTreeSnapshot>, ApiError> {
    Ok(Json(state.store().session_tree(&id).await?))
}

async fn message_bodies(
    State(state): State<AgentState>,
    SessionPath(id): SessionPath,
    JsonBody(request): JsonBody<MessageBodiesRequest>,
) -> Result<Json<MessageBodiesResponse>, ApiError> {
    let messages = state
        .store()
        .message_bodies(&id, &request.message_ids)
        .await?;
    Ok(Json(MessageBodiesResponse { messages }))
}

async fn submit_message(
    State(state): State<AgentState>,
    SessionPath(id): SessionPath,
    JsonBody(request): JsonBody<SubmitMessageRequest>,
) -> Result<(StatusCode, Json<QueueItem>), ApiError> {
    let text = MessageText::parse(&request.text)?;
    let item = state
        .store()
        .enqueue_message(&id, text, request.mode)
        .await?;
    state.engine().notify_session(id).await;
    Ok((StatusCode::CREATED, Json(item)))
}

async fn edit_user_message(
    State(state): State<AgentState>,
    MessagePath { session, message }: MessagePath,
    JsonBody(request): JsonBody<EditMessageRequest>,
) -> Result<(StatusCode, Json<durable::MessageSkeleton>), ApiError> {
    let text = MessageText::parse(&request.text)?;
    let node = state
        .store()
        .edit_user_message(&session, message, text)
        .await?;
    state.engine().notify_session(session).await;
    Ok((StatusCode::CREATED, Json(node.skeleton())))
}

async fn retry_assistant_message(
    State(state): State<AgentState>,
    MessagePath { session, message }: MessagePath,
) -> Result<StatusCode, ApiError> {
    state
        .store()
        .retry_assistant_message(&session, message)
        .await?;
    state.engine().notify_session(session).await;
    Ok(StatusCode::NO_CONTENT)
}

async fn switch_active_leaf(
    State(state): State<AgentState>,
    SessionPath(session): SessionPath,
    JsonBody(request): JsonBody<SwitchActiveLeafRequest>,
) -> Result<StatusCode, ApiError> {
    state
        .store()
        .switch_active_leaf(&session, request.message_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn cancel_run(
    State(state): State<AgentState>,
    SessionPath(id): SessionPath,
) -> Result<StatusCode, ApiError> {
    state.store().session_record(&id).await?;
    state.engine().cancel(&id).await;
    Ok(StatusCode::NO_CONTENT)
}

async fn remove_queue_item(
    State(state): State<AgentState>,
    QueueItemPath { session, item }: QueueItemPath,
) -> Result<StatusCode, ApiError> {
    state.store().remove_queue_item(&session, item).await?;
    Ok(StatusCode::NO_CONTENT)
}
