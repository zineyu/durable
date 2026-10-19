use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};

use crate::error::ApiError;
use crate::protocol::{
    BootstrapResponse, CreateSessionRequest, HealthResponse, HealthStatus,
    RegisterWorkspaceRequest, RemoveWorkspaceRequest, RenameSessionRequest, SessionMetadata,
    SessionTree, WorkspaceMetadata,
};
use crate::store::Store;

#[derive(Clone)]
pub struct AgentState {
    store: Arc<Store>,
    model_name: Arc<str>,
}

impl AgentState {
    pub fn new(store: Store, model_name: String) -> Self {
        Self {
            store: Arc::new(store),
            model_name: Arc::from(model_name.as_str()),
        }
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn model_name(&self) -> &str {
        &self.model_name
    }
}

pub fn router(state: AgentState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/bootstrap", get(bootstrap))
        .route("/api/workspaces", post(register_workspace))
        .route("/api/workspaces/remove", post(remove_workspace))
        .route("/api/sessions", post(create_session))
        .route("/api/sessions/{id}/tree", get(session_tree))
        .route("/api/sessions/{id}/rename", post(rename_session))
        .route("/api/sessions/{id}/delete", post(delete_session))
        .with_state(state)
}

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
    let snapshot = state.store().snapshot()?;
    Ok(Json(BootstrapResponse {
        model: state.model_name().to_owned(),
        max_event_id: snapshot.max_event_id,
        workspaces: snapshot.workspaces,
        sessions: snapshot.sessions,
    }))
}

async fn register_workspace(
    State(state): State<AgentState>,
    Json(request): Json<RegisterWorkspaceRequest>,
) -> Result<(StatusCode, Json<WorkspaceMetadata>), ApiError> {
    let workspace = state.store().register_workspace(&request.path)?;
    Ok((StatusCode::CREATED, Json(workspace)))
}

async fn remove_workspace(
    State(state): State<AgentState>,
    Json(request): Json<RemoveWorkspaceRequest>,
) -> Result<StatusCode, ApiError> {
    state.store().remove_workspace(&request.path)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn create_session(
    State(state): State<AgentState>,
    Json(request): Json<CreateSessionRequest>,
) -> Result<(StatusCode, Json<SessionMetadata>), ApiError> {
    let session = state
        .store()
        .create_session(&request.workspace, request.title.as_deref())?;
    Ok((StatusCode::CREATED, Json(session)))
}

async fn rename_session(
    State(state): State<AgentState>,
    Path(id): Path<String>,
    Json(request): Json<RenameSessionRequest>,
) -> Result<Json<SessionMetadata>, ApiError> {
    Ok(Json(state.store().rename_session(&id, &request.title)?))
}

async fn delete_session(
    State(state): State<AgentState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state.store().delete_session(&id)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn session_tree(
    State(state): State<AgentState>,
    Path(id): Path<String>,
) -> Result<Json<SessionTree>, ApiError> {
    Ok(Json(SessionTree {
        session: state.store().session(&id)?,
        active_leaf: None,
        nodes: Vec::new(),
    }))
}
