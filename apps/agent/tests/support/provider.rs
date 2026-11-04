//! 测试用脚本化 `OpenAI` 兼容 SSE 服务。它是真实 HTTP 服务，不替换被测代码。

#![expect(
    dead_code,
    clippy::expect_used,
    reason = "脚本化 provider 用 expect 直接报告夹具失败；本模块被每个测试目标各编译一次，未被该目标用到的条目属正常情况。"
)]

use std::collections::VecDeque;
use std::fmt::Write;
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
pub(crate) enum Script {
    /// 流式返回一段 assistant 文本。
    Text(String),
    /// 返回单个工具调用。
    ToolCall {
        id: String,
        name: String,
        arguments: String,
    },
    /// 同一条 assistant 消息返回多个工具调用。
    ToolCalls(Vec<ToolCallSpec>),
    /// 先流式返回文本，再保持连接不结束，用于观察运行中的 run。
    PartialThenHang(String),
    /// 返回非成功状态。`retry_after` 是 `Retry-After` 头的秒数。
    Fail {
        status: u16,
        retry_after: Option<u64>,
    },
    /// 永不响应，用于观察运行中的 run。
    Hang,
}

/// 脚本化的工具调用。
#[derive(Debug, Clone)]
pub(crate) struct ToolCallSpec {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

impl ToolCallSpec {
    pub(crate) fn new(id: &str, name: &str, arguments: &str) -> Self {
        Self {
            id: id.to_owned(),
            name: name.to_owned(),
            arguments: arguments.to_owned(),
        }
    }
}

#[derive(Clone)]
struct ScriptState {
    scripts: Arc<Mutex<VecDeque<Script>>>,
    requests: Arc<Mutex<Vec<Value>>>,
}

/// 本地脚本化 provider。
pub(crate) struct ScriptedProvider {
    address: SocketAddr,
    handle: tokio::task::JoinHandle<()>,
    state: ScriptState,
}

impl ScriptedProvider {
    pub(crate) async fn start(scripts: Vec<Script>) -> ScriptedProvider {
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

    pub(crate) fn base_url(&self) -> String {
        format!("http://{}/v1", self.address)
    }

    /// 追加一个脚本，供后续请求使用。
    pub(crate) fn push(&self, script: Script) {
        self.state
            .scripts
            .lock()
            .expect("脚本队列锁")
            .push_back(script);
    }

    /// 已收到的请求体。
    pub(crate) fn requests(&self) -> Vec<Value> {
        self.state.requests.lock().expect("请求日志锁").clone()
    }

    pub(crate) fn shutdown(self) {
        self.handle.abort();
    }
}

async fn chat_completions(State(state): State<ScriptState>, Json(body): Json<Value>) -> Response {
    state.requests.lock().expect("请求日志锁").push(body);
    let script = state.scripts.lock().expect("脚本队列锁").pop_front();
    match script {
        None => (StatusCode::INTERNAL_SERVER_ERROR, "没有可用脚本").into_response(),
        Some(Script::Fail {
            status,
            retry_after,
        }) => {
            let mut response = (
                StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                "上游错误",
            )
                .into_response();
            if let Some(seconds) = retry_after {
                response
                    .headers_mut()
                    .insert(header::RETRY_AFTER, seconds.into());
            }
            response
        }
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
            body.push_str(&tool_call_events(&[ToolCallSpec {
                id: id.clone(),
                name: name.clone(),
                arguments: arguments.clone(),
            }]));
        }
        Script::ToolCalls(calls) => {
            body.push_str(&tool_call_events(calls));
        }
        Script::Fail { .. } | Script::Hang | Script::PartialThenHang(_) => {}
    }
    body
}

fn tool_call_events(calls: &[ToolCallSpec]) -> String {
    let mut body = String::new();
    let headers: Vec<Value> = calls
        .iter()
        .enumerate()
        .map(|(index, call)| {
            json!({
                "index": index, "id": call.id, "type": "function",
                "function": { "name": call.name, "arguments": "" }
            })
        })
        .collect();
    write!(
        body,
        "data: {}\n\n",
        json!({ "choices": [{ "delta": { "tool_calls": headers } }] })
    )
    .expect("写入 tool_calls 头帧");
    let fragments: Vec<Value> = calls
        .iter()
        .enumerate()
        .map(|(index, call)| {
            json!({
                "index": index,
                "function": { "arguments": call.arguments }
            })
        })
        .collect();
    write!(
        body,
        "data: {}\n\n",
        json!({ "choices": [{ "delta": { "tool_calls": fragments } }] })
    )
    .expect("写入 tool_calls 参数帧");
    body.push_str("data: [DONE]\n\n");
    body
}

fn text_events(text: &str) -> String {
    let mut body = String::new();
    let characters: Vec<char> = text.chars().collect();
    for piece in characters.chunks(4) {
        let piece: String = piece.iter().collect();
        write!(
            body,
            "data: {}\n\n",
            json!({ "choices": [{ "delta": { "content": piece } }] })
        )
        .expect("写入文本帧");
    }
    body
}
