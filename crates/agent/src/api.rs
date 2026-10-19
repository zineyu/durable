use std::sync::Arc;

use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{FromRequest, FromRequestParts, Path, Request, State};
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::routing::{get, post};
use axum::{Json, Router};
use durable::{
    EventBus, MessageText, QueueItem, QueueItemId, SessionId, SessionTitle, Store, StoreError,
    WorkspacePath,
};
use serde::de::DeserializeOwned;

use crate::config::EngineConfig;
use crate::engine::RunEngine;
use crate::error::ApiError;
use crate::protocol::{
    BootstrapResponse, CreateSessionRequest, HealthResponse, HealthStatus,
    RegisterWorkspaceRequest, RemoveWorkspaceRequest, RenameSessionRequest, SubmitMessageRequest,
};
use crate::provider::ModelProvider;
use crate::sse;
use crate::tools::ToolBox;

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

/// `{id}/queue/{item}` 路径参数。
struct QueueItemPath {
    session: SessionId,
    item: QueueItemId,
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
    engine: Arc<RunEngine>,
    model_name: Arc<str>,
}

impl AgentState {
    /// 组装 agent 状态并执行启动恢复。
    ///
    /// 失败：恢复中断的 run 时存储读取或写入失败。
    pub async fn start(
        store: Store,
        model_name: String,
        provider: Arc<dyn ModelProvider>,
        tools: Arc<dyn ToolBox>,
        config: EngineConfig,
    ) -> Result<Self, StoreError> {
        let store = Arc::new(store);
        let engine = RunEngine::new(Arc::clone(&store), provider, tools, config);
        engine.recover().await?;
        Ok(Self {
            store,
            engine,
            model_name: Arc::from(model_name.as_str()),
        })
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn engine(&self) -> &Arc<RunEngine> {
        &self.engine
    }

    pub fn events(&self) -> &EventBus {
        self.store.events()
    }

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
        .route("/api/workspaces", post(register_workspace))
        .route("/api/workspaces/remove", post(remove_workspace))
        .route("/api/sessions", post(create_session))
        .route("/api/sessions/{id}/tree", get(session_tree))
        .route("/api/sessions/{id}/rename", post(rename_session))
        .route("/api/sessions/{id}/delete", post(delete_session))
        .route("/api/sessions/{id}/messages", post(submit_message))
        .route("/api/sessions/{id}/cancel", post(cancel_run))
        .route(
            "/api/sessions/{id}/queue/{item}/remove",
            post(remove_queue_item),
        )
        .with_state(state)
}

/// 在已绑定的监听器上提供 HTTP 服务，直到进程结束或出错。
pub async fn serve(listener: tokio::net::TcpListener, state: AgentState) -> std::io::Result<()> {
    axum::serve(listener, router(state)).await
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: HealthStatus::Ok,
        version: env!("CARGO_PKG_VERSION").to_owned(),
    })
}

async fn bootstrap(State(state): State<AgentState>) -> Result<Json<BootstrapResponse>, ApiError> {
    let snapshot = state.store().bootstrap().await?;
    Ok(Json(BootstrapResponse {
        model: state.model_name().to_owned(),
        max_event_id: snapshot.max_event_id,
        workspaces: snapshot.workspaces,
        sessions: snapshot.sessions,
    }))
}

async fn register_workspace(
    State(state): State<AgentState>,
    JsonBody(request): JsonBody<RegisterWorkspaceRequest>,
) -> Result<(StatusCode, Json<durable::WorkspaceMetadata>), ApiError> {
    let path = WorkspacePath::resolve(&request.path)?;
    let workspace = state.store().register_workspace(&path).await?;
    Ok((StatusCode::CREATED, Json(workspace)))
}

async fn remove_workspace(
    State(state): State<AgentState>,
    JsonBody(request): JsonBody<RemoveWorkspaceRequest>,
) -> Result<StatusCode, ApiError> {
    let path = WorkspacePath::parse(&request.path)?;
    state.store().remove_workspace(&path).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn create_session(
    State(state): State<AgentState>,
    JsonBody(request): JsonBody<CreateSessionRequest>,
) -> Result<(StatusCode, Json<durable::SessionMetadata>), ApiError> {
    let workspace = WorkspacePath::resolve(&request.workspace)?;
    let title = match request.title {
        Some(raw) => SessionTitle::parse(&raw)?,
        None => SessionTitle::default(),
    };
    let session = state.store().create_session(&workspace, title).await?;
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
) -> Result<Json<durable::SessionTree>, ApiError> {
    Ok(Json(state.store().session_tree(&id).await?))
}

async fn submit_message(
    State(state): State<AgentState>,
    SessionPath(id): SessionPath,
    JsonBody(request): JsonBody<SubmitMessageRequest>,
) -> Result<(StatusCode, Json<QueueItem>), ApiError> {
    let text = MessageText::parse(&request.text)?;
    let item = state.store().enqueue_message(&id, text).await?;
    state.engine().notify_session(id);
    Ok((StatusCode::CREATED, Json(item)))
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
