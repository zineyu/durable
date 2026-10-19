use std::fmt;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::protocol::{
    ErrorCode, ErrorResponse, SessionId, SessionIdError, SessionTitleError, WorkspacePath,
    WorkspacePathError,
};

#[derive(Debug)]
pub enum StoreError {
    Database(rusqlite::Error),
    Serialization(serde_json::Error),
    CorruptedData(String),
    WorkspaceDuplicate(WorkspacePath),
    WorkspaceNotFound(WorkspacePath),
    WorkspaceNotEmpty(WorkspacePath),
    SessionNotFound(SessionId),
}

impl fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(error) => write!(formatter, "SQLite 错误: {error}"),
            Self::Serialization(error) => write!(formatter, "事件序列化错误: {error}"),
            Self::CorruptedData(detail) => {
                write!(formatter, "数据库中的值不符合领域约束: {detail}")
            }
            Self::WorkspaceDuplicate(path) => {
                write!(formatter, "workspace 已注册: {}", path.as_str())
            }
            Self::WorkspaceNotFound(path) => {
                write!(formatter, "workspace 未注册: {}", path.as_str())
            }
            Self::WorkspaceNotEmpty(path) => {
                write!(formatter, "workspace 仍有 session: {}", path.as_str())
            }
            Self::SessionNotFound(id) => write!(formatter, "session 不存在: {}", id.as_str()),
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

    /// 请求体无法解析为协议类型。
    pub fn invalid_request(status: StatusCode, message: String) -> Self {
        Self::new(ErrorCode::InvalidRequest, status, message)
    }
}

impl From<WorkspacePathError> for ApiError {
    fn from(error: WorkspacePathError) -> Self {
        Self::new(
            ErrorCode::WorkspacePathInvalid,
            StatusCode::UNPROCESSABLE_ENTITY,
            error.to_string(),
        )
    }
}

impl From<SessionIdError> for ApiError {
    fn from(error: SessionIdError) -> Self {
        Self::new(
            ErrorCode::InvalidRequest,
            StatusCode::UNPROCESSABLE_ENTITY,
            error.to_string(),
        )
    }
}

impl From<SessionTitleError> for ApiError {
    fn from(error: SessionTitleError) -> Self {
        Self::new(
            ErrorCode::InvalidTitle,
            StatusCode::UNPROCESSABLE_ENTITY,
            error.to_string(),
        )
    }
}

impl From<StoreError> for ApiError {
    fn from(error: StoreError) -> Self {
        let (code, status) = match &error {
            StoreError::Database(_)
            | StoreError::Serialization(_)
            | StoreError::CorruptedData(_) => {
                (ErrorCode::Internal, StatusCode::INTERNAL_SERVER_ERROR)
            }
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
