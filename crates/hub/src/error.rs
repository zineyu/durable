//! hub 的 HTTP 错误信封：错误码、节点名与状态码来源。
//! 错误分类与结局类别由内核给出，本模块只把分类穷尽映射为 hub 的协议码。

use std::fmt;

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use durable::{NodeNameError, StoreError, StoreErrorClass, StoreErrorKind};

use crate::protocol::{HubErrorCode, HubErrorResponse};

#[derive(Debug)]
pub struct ApiError {
    code: HubErrorCode,
    message: String,
    status: StatusCode,
}

impl ApiError {
    /// 由内核错误分类构造信封。`From<StoreError>` 与分类表测试共用这一条路径。
    fn from_kind(kind: StoreErrorKind, message: String) -> Self {
        Self::new(code_of(kind), status_of(kind.class()), message)
    }

    fn new(code: HubErrorCode, status: StatusCode, message: String) -> Self {
        Self {
            code,
            message,
            status,
        }
    }

    /// 请求体或参数无法解析为协议类型。
    #[must_use]
    pub fn invalid_request(status: StatusCode, message: String) -> Self {
        Self::new(HubErrorCode::InvalidRequest, status, message)
    }

    /// 节点名不在节点表中。
    #[must_use]
    pub fn node_not_found(message: String) -> Self {
        Self::new(HubErrorCode::NodeNotFound, StatusCode::NOT_FOUND, message)
    }

    /// 节点不可达、协议版本不符，或转发失败。
    #[must_use]
    pub fn node_unavailable(message: String) -> Self {
        Self::new(
            HubErrorCode::NodeUnavailable,
            StatusCode::SERVICE_UNAVAILABLE,
            message,
        )
    }

    /// hub 自身的共享状态不可用。
    #[must_use]
    pub fn internal(message: String) -> Self {
        Self::new(
            HubErrorCode::Internal,
            StatusCode::INTERNAL_SERVER_ERROR,
            message,
        )
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ApiError {}

impl From<NodeNameError> for ApiError {
    fn from(error: NodeNameError) -> Self {
        Self::new(
            HubErrorCode::NodeNameInvalid,
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
///
/// hub 只写节点表，其余分类报 `internal`，不声称它认识那些结局。
fn code_of(kind: StoreErrorKind) -> HubErrorCode {
    match kind {
        StoreErrorKind::NodeNameTaken => HubErrorCode::NodeNameTaken,
        StoreErrorKind::NodeNotFound => HubErrorCode::NodeNotFound,
        StoreErrorKind::Database
        | StoreErrorKind::Migration
        | StoreErrorKind::DatabaseDirectory
        | StoreErrorKind::Serialization
        | StoreErrorKind::CorruptedData
        | StoreErrorKind::Invariant
        | StoreErrorKind::WorkspaceDuplicate
        | StoreErrorKind::WorkspaceNotFound
        | StoreErrorKind::WorkspaceNotEmpty
        | StoreErrorKind::SessionNotFound
        | StoreErrorKind::QueueItemNotFound
        | StoreErrorKind::MessageNotFound
        | StoreErrorKind::MessageRoleMismatch
        | StoreErrorKind::RunInProgress
        | StoreErrorKind::QueueNotEmpty => HubErrorCode::Internal,
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
        let body = HubErrorResponse {
            code: self.code,
            message: self.message,
        };
        (self.status, Json(body)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use axum::response::IntoResponse;
    use durable::{StoreError, StoreErrorClass, StoreErrorKind};

    use super::{ApiError, code_of, status_of};
    use crate::protocol::HubErrorCode;

    /// 分类到协议码与状态码的独立事实源。
    ///
    /// hub 只有节点表的写命令，因此节点以外的分类统一报 `internal`；
    /// 状态码仍取自内核的类别。
    const TABLE: [(StoreErrorKind, HubErrorCode, u16); 17] = [
        (StoreErrorKind::Database, HubErrorCode::Internal, 500),
        (StoreErrorKind::Migration, HubErrorCode::Internal, 500),
        (
            StoreErrorKind::DatabaseDirectory,
            HubErrorCode::Internal,
            500,
        ),
        (StoreErrorKind::Serialization, HubErrorCode::Internal, 500),
        (StoreErrorKind::CorruptedData, HubErrorCode::Internal, 500),
        (StoreErrorKind::Invariant, HubErrorCode::Internal, 500),
        (
            StoreErrorKind::WorkspaceDuplicate,
            HubErrorCode::Internal,
            409,
        ),
        (
            StoreErrorKind::WorkspaceNotFound,
            HubErrorCode::Internal,
            404,
        ),
        (
            StoreErrorKind::WorkspaceNotEmpty,
            HubErrorCode::Internal,
            409,
        ),
        (StoreErrorKind::SessionNotFound, HubErrorCode::Internal, 404),
        (
            StoreErrorKind::QueueItemNotFound,
            HubErrorCode::Internal,
            404,
        ),
        (StoreErrorKind::MessageNotFound, HubErrorCode::Internal, 404),
        (
            StoreErrorKind::MessageRoleMismatch,
            HubErrorCode::Internal,
            422,
        ),
        (StoreErrorKind::RunInProgress, HubErrorCode::Internal, 409),
        (StoreErrorKind::QueueNotEmpty, HubErrorCode::Internal, 409),
        (
            StoreErrorKind::NodeNameTaken,
            HubErrorCode::NodeNameTaken,
            409,
        ),
        (
            StoreErrorKind::NodeNotFound,
            HubErrorCode::NodeNotFound,
            404,
        ),
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

    /// ADR 0043：`ApiError` 的状态码来自内核的结局类别，不由 hub 决定。
    #[test]
    fn api_error_takes_status_from_the_kernel_class() {
        let error =
            StoreError::NodeNameTaken(durable::NodeName::parse("local").expect("合法节点名"));
        assert_eq!(error.kind().class(), StoreErrorClass::Conflict);
        let response = ApiError::from(error).into_response();
        assert_eq!(response.status(), status_of(StoreErrorClass::Conflict));
    }
}
