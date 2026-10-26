//! HTTP 错误信封：状态码与错误码的映射。
//! 分类与结局类别由内核给出（`durable::StoreErrorKind`、`durable::StoreErrorClass`），
//! 本模块只把分类穷尽映射为协议码，不再自行判断错误性质。

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use durable::{
    MessageIdError, MessageTextError, QueueItemIdError, SessionIdError, SessionTitleError,
    StoreError, StoreErrorClass, StoreErrorKind, WorkspacePathError,
};

use crate::protocol::{ErrorCode, ErrorResponse};

#[derive(Debug)]
pub struct ApiError {
    code: ErrorCode,
    message: String,
    status: StatusCode,
}

impl ApiError {
    /// 由内核错误分类构造信封。`From<StoreError>` 与分类表测试共用这一条路径。
    fn from_kind(kind: StoreErrorKind, message: String) -> Self {
        Self::new(code_of(kind), status_of(kind.class()), message)
    }

    fn new(code: ErrorCode, status: StatusCode, message: String) -> Self {
        Self {
            code,
            message,
            status,
        }
    }

    /// 请求体无法解析为协议类型。
    #[must_use]
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

impl From<QueueItemIdError> for ApiError {
    fn from(error: QueueItemIdError) -> Self {
        Self::new(
            ErrorCode::InvalidRequest,
            StatusCode::UNPROCESSABLE_ENTITY,
            error.to_string(),
        )
    }
}

impl From<MessageIdError> for ApiError {
    fn from(error: MessageIdError) -> Self {
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
        Self::from_kind(error.kind(), error.to_string())
    }
}

/// 分类到协议码。穷尽匹配：新增分类时编译器要求在这里给出协议码。
fn code_of(kind: StoreErrorKind) -> ErrorCode {
    match kind {
        StoreErrorKind::Database
        | StoreErrorKind::Migration
        | StoreErrorKind::DatabaseDirectory
        | StoreErrorKind::Serialization
        | StoreErrorKind::CorruptedData
        | StoreErrorKind::Invariant => ErrorCode::Internal,
        StoreErrorKind::WorkspaceDuplicate => ErrorCode::WorkspaceDuplicate,
        StoreErrorKind::WorkspaceNotFound => ErrorCode::WorkspaceNotFound,
        StoreErrorKind::WorkspaceNotEmpty => ErrorCode::WorkspaceNotEmpty,
        StoreErrorKind::SessionNotFound => ErrorCode::SessionNotFound,
        StoreErrorKind::QueueItemNotFound => ErrorCode::QueueItemNotFound,
        StoreErrorKind::MessageNotFound => ErrorCode::MessageNotFound,
        StoreErrorKind::MessageRoleMismatch => ErrorCode::MessageRoleMismatch,
        StoreErrorKind::RunInProgress => ErrorCode::RunInProgress,
        StoreErrorKind::QueueNotEmpty => ErrorCode::QueueNotEmpty,
        StoreErrorKind::NodeNameTaken => ErrorCode::NodeNameTaken,
        StoreErrorKind::NodeNotFound => ErrorCode::NodeNotFound,
    }
}

/// 结局类别到 HTTP 状态码。状态码只由内核的结局类别决定。
fn status_of(class: StoreErrorClass) -> StatusCode {
    match class {
        StoreErrorClass::NotFound => StatusCode::NOT_FOUND,
        StoreErrorClass::Conflict => StatusCode::CONFLICT,
        StoreErrorClass::InvalidArgument => StatusCode::UNPROCESSABLE_ENTITY,
        StoreErrorClass::Internal | StoreErrorClass::StorageFailure => {
            StatusCode::INTERNAL_SERVER_ERROR
        }
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

#[cfg(test)]
mod tests {
    use axum::response::IntoResponse;
    use durable::{StoreError, StoreErrorClass, StoreErrorKind};

    use super::{ApiError, code_of, status_of};
    use crate::protocol::ErrorCode;

    /// 分类到协议码与状态码的独立事实源。
    const TABLE: [(StoreErrorKind, ErrorCode, u16); 17] = [
        (StoreErrorKind::Database, ErrorCode::Internal, 500),
        (StoreErrorKind::Migration, ErrorCode::Internal, 500),
        (StoreErrorKind::DatabaseDirectory, ErrorCode::Internal, 500),
        (StoreErrorKind::Serialization, ErrorCode::Internal, 500),
        (StoreErrorKind::CorruptedData, ErrorCode::Internal, 500),
        (StoreErrorKind::Invariant, ErrorCode::Internal, 500),
        (
            StoreErrorKind::WorkspaceDuplicate,
            ErrorCode::WorkspaceDuplicate,
            409,
        ),
        (
            StoreErrorKind::WorkspaceNotFound,
            ErrorCode::WorkspaceNotFound,
            404,
        ),
        (
            StoreErrorKind::WorkspaceNotEmpty,
            ErrorCode::WorkspaceNotEmpty,
            409,
        ),
        (
            StoreErrorKind::SessionNotFound,
            ErrorCode::SessionNotFound,
            404,
        ),
        (
            StoreErrorKind::QueueItemNotFound,
            ErrorCode::QueueItemNotFound,
            404,
        ),
        (
            StoreErrorKind::MessageNotFound,
            ErrorCode::MessageNotFound,
            404,
        ),
        (
            StoreErrorKind::MessageRoleMismatch,
            ErrorCode::MessageRoleMismatch,
            422,
        ),
        (StoreErrorKind::RunInProgress, ErrorCode::RunInProgress, 409),
        (StoreErrorKind::QueueNotEmpty, ErrorCode::QueueNotEmpty, 409),
        (StoreErrorKind::NodeNameTaken, ErrorCode::NodeNameTaken, 409),
        (StoreErrorKind::NodeNotFound, ErrorCode::NodeNotFound, 404),
    ];

    /// ADR 0043：分类到协议码与状态码的映射穷尽，状态码取自结局类别。
    #[test]
    fn maps_every_kind_to_its_code_and_its_class_status() {
        for (kind, code, status) in TABLE {
            assert_eq!(code_of(kind), code, "{kind:?}");
            let response = ApiError::from_kind(kind, String::new()).into_response();
            assert_eq!(
                response.status().as_u16(),
                status,
                "{kind:?} 的状态码取自 {:?}",
                kind.class()
            );
        }
    }

    /// ADR 0043：`ApiError` 的状态码来自内核的结局类别，不由 agent 决定。
    #[test]
    fn api_error_takes_status_from_the_kernel_class() {
        let error = StoreError::DatabaseDirectory {
            path: std::path::PathBuf::from("/nowhere"),
            message: "测试".to_owned(),
        };
        assert_eq!(error.kind().class(), StoreErrorClass::StorageFailure);
        let response = ApiError::from(error).into_response();
        assert_eq!(
            response.status(),
            status_of(StoreErrorClass::StorageFailure)
        );
    }
}
