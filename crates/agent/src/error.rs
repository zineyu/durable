use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use durable::{
    MessageTextError, SessionIdError, SessionTitleError, StoreError, WorkspacePathError,
};

use crate::protocol::{ErrorCode, ErrorResponse};

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

impl From<MessageTextError> for ApiError {
    fn from(error: MessageTextError) -> Self {
        Self::new(
            ErrorCode::InvalidMessage,
            StatusCode::UNPROCESSABLE_ENTITY,
            error.to_string(),
        )
    }
}

impl From<StoreError> for ApiError {
    fn from(error: StoreError) -> Self {
        let (code, status) = match &error {
            StoreError::Database(_)
            | StoreError::Migration(_)
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
