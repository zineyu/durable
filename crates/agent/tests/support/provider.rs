//! 测试用脚本化 OpenAI 兼容 SSE 服务。它是真实 HTTP 服务，不替换被测代码。

#![allow(dead_code)]

use std::collections::VecDeque;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};

use axum::Json;
use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use futures_util::StreamExt;
use serde_json::{Value, json};

/// 一次请求的脚本。
#[derive(Debug, Clone)]
pub enum Script {
    /// 流式返回一段 assistant 文本。
    Text(String),
    /// 返回单个工具调用。
    ToolCall {
        id: String,
        name: String,
        arguments: String,
    },
    /// 先流式返回文本，再保持连接不结束，用于观察运行中的 run。
    PartialThenHang(String),
    /// 返回非成功状态。
    Fail(u16),
    /// 永不响应，用于观察运行中的 run。
    Hang,
}

#[derive(Clone)]
struct ScriptState {
    scripts: Arc<Mutex<VecDeque<Script>>>,
    requests: Arc<Mutex<Vec<Value>>>,
}

/// 本地脚本化 provider。
pub struct ScriptedProvider {
    address: SocketAddr,
    handle: tokio::task::JoinHandle<()>,
    state: ScriptState,
}

impl ScriptedProvider {
    pub async fn start(scripts: Vec<Script>) -> ScriptedProvider {
        let state = ScriptState {
            scripts: Arc::new(Mutex::new(scripts.into())),
            requests: Arc::new(Mutex::new(Vec::new())),
        };
        let listener = tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .await
            .expect("绑定 scripted provider 端口");
        let address = listener.local_addr().expect("读取 scripted provider 地址");
        let router = axum::Router::new()
            .route("/v1/chat/completions", post(chat_completions))
            .with_state(state.clone());
        let handle = tokio::spawn(async move {
            axum::serve(listener, router)
                .await
                .expect("scripted provider 运行");
        });
        ScriptedProvider {
            address,
            handle,
            state,
        }
    }

    pub fn base_url(&self) -> String {
        format!("http://{}/v1", self.address)
    }

    /// 追加一个脚本，供后续请求使用。
    pub fn push(&self, script: Script) {
        self.state
            .scripts
            .lock()
            .expect("脚本队列锁")
            .push_back(script);
    }

    /// 已收到的请求体。
    pub fn requests(&self) -> Vec<Value> {
        self.state.requests.lock().expect("请求日志锁").clone()
    }

    pub fn shutdown(self) {
        self.handle.abort();
    }
}

async fn chat_completions(State(state): State<ScriptState>, Json(body): Json<Value>) -> Response {
    state.requests.lock().expect("请求日志锁").push(body);
    let script = state.scripts.lock().expect("脚本队列锁").pop_front();
    match script {
        None => (StatusCode::INTERNAL_SERVER_ERROR, "没有可用脚本").into_response(),
        Some(Script::Fail(status)) => (
            StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            "上游错误",
        )
            .into_response(),
        Some(Script::Hang) => std::future::pending::<Response>().await,
        Some(Script::PartialThenHang(text)) => {
            let first = Bytes::from(text_events(&text));
            let stream =
                futures_util::stream::once(async move { Ok::<Bytes, std::io::Error>(first) })
                    .chain(futures_util::stream::pending());
            (
                [(header::CONTENT_TYPE, "text/event-stream")],
                Body::from_stream(stream),
            )
                .into_response()
        }
        Some(script) => (
            [(header::CONTENT_TYPE, "text/event-stream")],
            sse_body(&script),
        )
            .into_response(),
    }
}

fn sse_body(script: &Script) -> String {
    let mut body = String::new();
    match script {
        Script::Text(text) => {
            body.push_str(&text_events(text));
            body.push_str("data: [DONE]\n\n");
        }
        Script::ToolCall {
            id,
            name,
            arguments,
        } => {
            body.push_str(&format!(
                "data: {}\n\n",
                json!({ "choices": [{ "delta": { "tool_calls": [{
                    "index": 0, "id": id, "type": "function",
                    "function": { "name": name, "arguments": "" }
                }] } }] })
            ));
            body.push_str(&format!(
                "data: {}\n\n",
                json!({ "choices": [{ "delta": { "tool_calls": [{
                    "index": 0, "function": { "arguments": arguments }
                }] } }] })
            ));
            body.push_str("data: [DONE]\n\n");
        }
        Script::Fail(_) | Script::Hang | Script::PartialThenHang(_) => {}
    }
    body
}

fn text_events(text: &str) -> String {
    let mut body = String::new();
    let characters: Vec<char> = text.chars().collect();
    for piece in characters.chunks(4) {
        let piece: String = piece.iter().collect();
        body.push_str(&format!(
            "data: {}\n\n",
            json!({ "choices": [{ "delta": { "content": piece } }] })
        ));
    }
    body
}
