use std::fmt;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::protocol::{ErrorCode, ErrorResponse};

#[derive(Debug)]
pub enum StoreError {
    Database(rusqlite::Error),
    Serialization(serde_json::Error),
    WorkspacePathInvalid(String),
    WorkspaceDuplicate(String),
    WorkspaceNotFound(String),
    WorkspaceNotEmpty(String),
    SessionNotFound(String),
    InvalidTitle,
}

impl fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(error) => write!(formatter, "SQLite 错误: {error}"),
            Self::Serialization(error) => write!(formatter, "事件序列化错误: {error}"),
            Self::WorkspacePathInvalid(path) => write!(formatter, "workspace 路径无效: {path}"),
            Self::WorkspaceDuplicate(path) => write!(formatter, "workspace 已注册: {path}"),
            Self::WorkspaceNotFound(path) => write!(formatter, "workspace 未注册: {path}"),
            Self::WorkspaceNotEmpty(path) => write!(formatter, "workspace 仍有 session: {path}"),
            Self::SessionNotFound(id) => write!(formatter, "session 不存在: {id}"),
            Self::InvalidTitle => write!(formatter, "session 标题不能为空"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<rusqlite::Error> for StoreError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Database(error)
    }
}

impl From<serde_json::Error> for StoreError {
    fn from(error: serde_json::Error) -> Self {
        Self::Serialization(error)
    }
}

#[derive(Debug)]
pub struct ApiError {
    code: ErrorCode,
    message: String,
    status: StatusCode,
}

impl ApiError {
    fn new(code: ErrorCode, status: StatusCode, message: String) -> Self {
        Self {
            code,
            message,
            status,
        }
    }
}

impl From<StoreError> for ApiError {
    fn from(error: StoreError) -> Self {
        let (code, status) = match &error {
            StoreError::Database(_) | StoreError::Serialization(_) => {
                (ErrorCode::Internal, StatusCode::INTERNAL_SERVER_ERROR)
            }
            StoreError::WorkspacePathInvalid(_) => (
                ErrorCode::WorkspacePathInvalid,
                StatusCode::UNPROCESSABLE_ENTITY,
            ),
            StoreError::InvalidTitle => (ErrorCode::InvalidTitle, StatusCode::UNPROCESSABLE_ENTITY),
            StoreError::WorkspaceDuplicate(_) => {
                (ErrorCode::WorkspaceDuplicate, StatusCode::CONFLICT)
            }
            StoreError::WorkspaceNotEmpty(_) => {
                (ErrorCode::WorkspaceNotEmpty, StatusCode::CONFLICT)
            }
            StoreError::WorkspaceNotFound(_) => {
                (ErrorCode::WorkspaceNotFound, StatusCode::NOT_FOUND)
            }
            StoreError::SessionNotFound(_) => (ErrorCode::SessionNotFound, StatusCode::NOT_FOUND),
        };
        Self::new(code, status, error.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorResponse {
                code: self.code,
                message: self.message,
            }),
        )
            .into_response()
    }
}
