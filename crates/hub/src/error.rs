use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use durable::{NodeNameError, StoreError};

use crate::protocol::{HubErrorCode, HubErrorResponse};

#[derive(Debug)]
pub struct ApiError {
    code: HubErrorCode,
    message: String,
    status: StatusCode,
}

impl ApiError {
    fn new(code: HubErrorCode, status: StatusCode, message: String) -> Self {
        Self {
            code,
            message,
            status,
        }
    }

    /// 请求体或参数无法解析为协议类型。
    pub fn invalid_request(status: StatusCode, message: String) -> Self {
        Self::new(HubErrorCode::InvalidRequest, status, message)
    }
}

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
        let code = match error {
            StoreError::NodeNameTaken(_) => HubErrorCode::NodeNameTaken,
            StoreError::NodeNotFound(_) => HubErrorCode::UnknownNode,
            _ => HubErrorCode::Internal,
        };
        let status = match code {
            HubErrorCode::NodeNameTaken => StatusCode::CONFLICT,
            HubErrorCode::UnknownNode => StatusCode::NOT_FOUND,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        Self::new(code, status, error.to_string())
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
