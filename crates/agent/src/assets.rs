//! 浏览器入口的静态资源：编译期内嵌 `client/dist`，未知非 API 路径回落到 `index.html`。
//! API 路由在 `api`，错误信封在 `error`。

use axum::http::{HeaderValue, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use rust_embed::Embed;

use crate::error::ApiError;

#[derive(Embed)]
#[folder = "$CARGO_MANIFEST_DIR/../../client/dist"]
struct Assets;

/// 静态资源兜底：已知资源按路径返回，`/api/*` 之外的未知路径回落到 `index.html`。
pub(crate) async fn static_asset(uri: Uri) -> Response {
    let requested = uri.path().trim_start_matches('/');
    if requested == "api" || requested.starts_with("api/") {
        return ApiError::not_found(format!("接口不存在: /{requested}")).into_response();
    }
    let path = if requested.is_empty() {
        "index.html"
    } else {
        requested
    };
    match Assets::get(path) {
        Some(asset) => asset_response(path, asset.data.into_owned()),
        None => match Assets::get("index.html") {
            Some(index) => asset_response("index.html", index.data.into_owned()),
            None => (StatusCode::NOT_FOUND, "资源不存在").into_response(),
        },
    }
}

fn asset_response(path: &str, data: Vec<u8>) -> Response {
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let content_type = HeaderValue::from_str(mime.as_ref())
        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));
    ([(header::CONTENT_TYPE, content_type)], data).into_response()
}
