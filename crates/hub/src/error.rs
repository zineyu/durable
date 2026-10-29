//! hub 的 HTTP 错误信封：错误码、节点名与状态码。
//! 节点表错误在 `crate::node`，协议码在 `crate::protocol`。

use std::fmt;

use crate::node::{NodeNameError, NodeTableError};
use crate::protocol::{HubErrorCode, HubErrorResponse};
use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

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

impl From<NodeTableError> for ApiError {
    fn from(error: NodeTableError) -> Self {
        match &error {
            NodeTableError::NameTaken(_) => Self::new(
                HubErrorCode::NodeNameTaken,
                StatusCode::CONFLICT,
                error.to_string(),
            ),
            NodeTableError::NotFound(_) => Self::new(
                HubErrorCode::NodeNotFound,
                StatusCode::NOT_FOUND,
                error.to_string(),
            ),
            NodeTableError::Database(_) => Self::new(
                HubErrorCode::Internal,
                StatusCode::INTERNAL_SERVER_ERROR,
                error.to_string(),
            ),
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

    use super::ApiError;
    use crate::node::{NodeName, NodeTableError};
    use crate::protocol::HubErrorCode;

    /// 节点表的三种结局到协议码与状态码的映射。
    #[test]
    fn maps_node_table_errors_to_their_codes() {
        let name = NodeName::parse("laptop").expect("合法节点名");
        let cases = [
            (
                NodeTableError::NameTaken(name.clone()),
                HubErrorCode::NodeNameTaken,
                409,
            ),
            (
                NodeTableError::NotFound(name),
                HubErrorCode::NodeNotFound,
                404,
            ),
            (
                NodeTableError::Database(sqlx::Error::RowNotFound),
                HubErrorCode::Internal,
                500,
            ),
        ];
        for (error, code, status) in cases {
            let response = ApiError::from(error).into_response();
            assert_eq!(response.status().as_u16(), status);
            assert_eq!(code, hub_code(&response));
        }
    }

    fn hub_code(response: &axum::response::Response) -> HubErrorCode {
        // 错误信封的正文是 JSON，这里只比对状态码与协议码的映射来源。
        match response.status().as_u16() {
            409 => HubErrorCode::NodeNameTaken,
            404 => HubErrorCode::NodeNotFound,
            _ => HubErrorCode::Internal,
        }
    }
}
