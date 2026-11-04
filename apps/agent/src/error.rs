//! HTTP 错误信封：状态码与错误码的映射。
//! 结局类别由内核给出（`durable::StoreErrorClass`），本模块把变体穷尽映射为协议码，
//! 不再自行判断错误性质。

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use durable::link::WorkspaceCheckError;
use durable::{
    HostNameError, MessageIdError, MessageTextError, QueueItemIdError, SessionIdError,
    SessionTitleError, StoreError, StoreErrorClass, WorkspacePathError,
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
    #[must_use]
    pub fn invalid_request(status: StatusCode, message: String) -> Self {
        Self::new(ErrorCode::InvalidRequest, status, message)
    }

    /// 未知路径。API 作用域的未知路径以 JSON 信封返回 404，而不是回落静态资源。
    #[must_use]
    pub fn not_found(message: String) -> Self {
        Self::new(ErrorCode::InvalidRequest, StatusCode::NOT_FOUND, message)
    }

    /// 请求里的 host 不在 host 表中。
    #[must_use]
    pub fn host_not_found(message: String) -> Self {
        Self::new(ErrorCode::HostNotFound, StatusCode::NOT_FOUND, message)
    }

    /// host 当前不可用：停用、不可达或版本不符。
    #[must_use]
    pub fn host_unavailable(message: String) -> Self {
        Self::new(ErrorCode::HostUnavailable, StatusCode::CONFLICT, message)
    }

    /// `ssh` 种类的 host 缺少目标字符串。
    #[must_use]
    pub fn host_invalid_target(message: String) -> Self {
        Self::new(
            ErrorCode::HostInvalidTarget,
            StatusCode::UNPROCESSABLE_ENTITY,
            message,
        )
    }

    /// host 上判定的路径错误。
    #[must_use]
    pub fn workspace_invalid(message: String) -> Self {
        Self::new(
            ErrorCode::WorkspacePathInvalid,
            StatusCode::UNPROCESSABLE_ENTITY,
            message,
        )
    }
}

impl From<HostNameError> for ApiError {
    fn from(error: HostNameError) -> Self {
        Self::new(
            ErrorCode::HostNameInvalid,
            StatusCode::UNPROCESSABLE_ENTITY,
            error.to_string(),
        )
    }
}

/// executor 判定的路径错误。文件系统事实只有 host 判定，agent 只做映射。
impl From<WorkspaceCheckError> for ApiError {
    fn from(error: WorkspaceCheckError) -> Self {
        Self::workspace_invalid(match error {
            WorkspaceCheckError::NotAbsolute => "workspace 路径必须是绝对路径".to_owned(),
            WorkspaceCheckError::NotUtf8 => "workspace 路径不是 UTF-8".to_owned(),
            WorkspaceCheckError::NotFound => "workspace 路径不存在".to_owned(),
            WorkspaceCheckError::Unreadable => "workspace 路径无法读取".to_owned(),
            WorkspaceCheckError::NotDirectory => "workspace 路径不是目录".to_owned(),
        })
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
        Self::new(code_of(&error), status_of(error.class()), error.to_string())
    }
}

/// 变体到协议码。穷尽匹配：新增变体时编译器要求在这里给出协议码。
fn code_of(error: &StoreError) -> ErrorCode {
    match error {
        StoreError::Database(_)
        | StoreError::Migration(_)
        | StoreError::DatabaseDirectory { .. }
        | StoreError::Serialization(_)
        | StoreError::CorruptedData(_)
        | StoreError::LegacyDatabase(_)
        | StoreError::Invariant(_) => ErrorCode::Internal,
        StoreError::WorkspaceDuplicate(_) => ErrorCode::WorkspaceDuplicate,
        StoreError::WorkspaceNotFound(_) => ErrorCode::WorkspaceNotFound,
        StoreError::WorkspaceNotEmpty(_) => ErrorCode::WorkspaceNotEmpty,
        StoreError::SessionNotFound(_) => ErrorCode::SessionNotFound,
        StoreError::QueueItemNotFound(_, _) => ErrorCode::QueueItemNotFound,
        StoreError::MessageNotFound(_, _) => ErrorCode::MessageNotFound,
        StoreError::MessageRoleMismatch(_, _) => ErrorCode::MessageRoleMismatch,
        StoreError::RunInProgress(_) => ErrorCode::RunInProgress,
        StoreError::QueueNotEmpty(_) => ErrorCode::QueueNotEmpty,
        StoreError::HostNameTaken(_) => ErrorCode::HostNameTaken,
        StoreError::HostNotFound(_) => ErrorCode::HostNotFound,
        StoreError::HostNotEmpty(_, _) => ErrorCode::HostInUse,
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
    use durable::{
        HostName, MessageId, MessageRole, QueueItemId, SessionId, StoreError, StoreErrorClass,
        WorkspacePath,
    };

    use super::{ApiError, code_of, status_of};
    use crate::protocol::ErrorCode;

    /// 归到 `Internal` 的变体：协议码不区分它们的差别。
    fn internal_errors() -> Vec<(StoreError, ErrorCode, u16)> {
        vec![
            (
                StoreError::Database(sqlx::Error::RowNotFound),
                ErrorCode::Internal,
                500,
            ),
            (
                StoreError::Migration(sqlx::migrate::MigrateError::VersionMissing(1)),
                ErrorCode::Internal,
                500,
            ),
            (
                StoreError::DatabaseDirectory {
                    path: "/nowhere".into(),
                    message: "测试".to_owned(),
                },
                ErrorCode::Internal,
                500,
            ),
            (
                StoreError::Serialization(
                    serde_json::from_str::<i32>("不是 JSON").expect_err("解析失败"),
                ),
                ErrorCode::Internal,
                500,
            ),
            (
                StoreError::CorruptedData("message_part.body".to_owned()),
                ErrorCode::Internal,
                500,
            ),
            (
                StoreError::LegacyDatabase("/nowhere/old.sqlite".into()),
                ErrorCode::Internal,
                500,
            ),
            (
                StoreError::Invariant("测试".to_owned()),
                ErrorCode::Internal,
                500,
            ),
        ]
    }

    /// 各自有专属协议码的变体。
    fn mapped_errors() -> Vec<(StoreError, ErrorCode, u16)> {
        let workspace = WorkspacePath::parse("/workspace").expect("合法 workspace 路径");
        let session = SessionId::parse("session").expect("合法 session id");
        let message = MessageId::parse(7).expect("合法消息 id");
        let queue_item = QueueItemId::parse(3).expect("合法队列项 id");
        let host = HostName::parse("local").expect("合法 host 名");
        vec![
            (
                StoreError::WorkspaceDuplicate(workspace.clone()),
                ErrorCode::WorkspaceDuplicate,
                409,
            ),
            (
                StoreError::WorkspaceNotFound(workspace.clone()),
                ErrorCode::WorkspaceNotFound,
                404,
            ),
            (
                StoreError::WorkspaceNotEmpty(workspace),
                ErrorCode::WorkspaceNotEmpty,
                409,
            ),
            (
                StoreError::SessionNotFound(session.clone()),
                ErrorCode::SessionNotFound,
                404,
            ),
            (
                StoreError::QueueItemNotFound(session.clone(), queue_item),
                ErrorCode::QueueItemNotFound,
                404,
            ),
            (
                StoreError::MessageNotFound(session.clone(), message),
                ErrorCode::MessageNotFound,
                404,
            ),
            (
                StoreError::MessageRoleMismatch(message, MessageRole::User),
                ErrorCode::MessageRoleMismatch,
                422,
            ),
            (
                StoreError::RunInProgress(session.clone()),
                ErrorCode::RunInProgress,
                409,
            ),
            (
                StoreError::QueueNotEmpty(session),
                ErrorCode::QueueNotEmpty,
                409,
            ),
            (
                StoreError::HostNameTaken(host.clone()),
                ErrorCode::HostNameTaken,
                409,
            ),
            (
                StoreError::HostNotFound(host.clone()),
                ErrorCode::HostNotFound,
                404,
            ),
            (StoreError::HostNotEmpty(host, 2), ErrorCode::HostInUse, 409),
        ]
    }

    /// 每个变体的协议码与状态码。这是独立事实源：新增变体时必须在这里表态。
    fn every_error() -> Vec<(StoreError, ErrorCode, u16)> {
        let mut all = internal_errors();
        all.extend(mapped_errors());
        all
    }

    /// ADR 0043 与 ADR 0054：变体到协议码与状态码的映射穷尽，状态码取自结局类别。
    #[test]
    fn maps_every_error_to_its_code_and_its_class_status() {
        for (error, code, status) in every_error() {
            assert_eq!(code_of(&error), code, "{error}");
            let response = ApiError::from(error).into_response();
            assert_eq!(
                response.status().as_u16(),
                status,
                "状态码取自变体的结局类别"
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
        assert_eq!(error.class(), StoreErrorClass::StorageFailure);
        let response = ApiError::from(error).into_response();
        assert_eq!(
            response.status(),
            status_of(StoreErrorClass::StorageFailure)
        );
    }
}
