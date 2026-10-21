use std::fmt;
use std::time::Duration;

use async_trait::async_trait;
use futures_util::{StreamExt, stream::BoxStream};

use crate::config::ModelConfig;
use crate::tools::ToolDefinition;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    System,
    User,
    Assistant,
    Tool,
}

/// 一轮请求中的单条消息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
    pub tool_calls: Vec<ChatToolCall>,
    pub tool_call_id: Option<String>,
}

impl ChatMessage {
    pub fn text(role: ChatRole, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }

    fn to_json(&self) -> serde_json::Value {
        let mut value = serde_json::json!({ "role": self.role, "content": self.content });
        if !self.tool_calls.is_empty() {
            value["tool_calls"] = serde_json::Value::Array(
                self.tool_calls
                    .iter()
                    .map(|call| {
                        serde_json::json!({
                            "id": call.id,
                            "type": "function",
                            "function": { "name": call.name, "arguments": call.arguments },
                        })
                    })
                    .collect(),
            );
        }
        if let Some(tool_call_id) = &self.tool_call_id {
            value["tool_call_id"] = serde_json::Value::String(tool_call_id.clone());
        }
        value
    }
}

/// assistant 请求的工具调用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// 流式响应片段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatChunk {
    Text(String),
    ToolCall {
        index: usize,
        id: Option<String>,
        name: Option<String>,
        arguments: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderError {
    Request(String),
    /// 上游返回非成功状态。`retry_after` 来自 `Retry-After` 头，只解释秒数形式。
    Status {
        status: u16,
        message: String,
        retry_after: Option<Duration>,
    },
    Decode(String),
    Upstream(String),
}

impl ProviderError {
    /// 限流：`429`，或供应商给了等待建议。
    pub fn is_rate_limited(&self) -> bool {
        match self {
            Self::Status {
                status,
                retry_after,
                ..
            } => *status == 429 || retry_after.is_some(),
            _ => false,
        }
    }

    /// 供应商建议的等待时长。不限流或没给建议时为 `None`。
    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::Status { retry_after, .. } => *retry_after,
            _ => None,
        }
    }
}

impl fmt::Display for ProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Request(message) => write!(formatter, "模型请求失败: {message}"),
            Self::Status {
                status,
                message,
                retry_after,
            } => {
                write!(formatter, "模型返回非成功状态 {status}: {message}")?;
                if let Some(delay) = retry_after {
                    write!(
                        formatter,
                        "（供应商建议等待 {} 毫秒后重试）",
                        delay.as_millis()
                    )?;
                }
                Ok(())
            }
            Self::Decode(message) => write!(formatter, "模型响应解析失败: {message}"),
            Self::Upstream(message) => write!(formatter, "模型上游错误: {message}"),
        }
    }
}

impl std::error::Error for ProviderError {}

pub type ChatStream = BoxStream<'static, Result<ChatChunk, ProviderError>>;

/// 发起一轮带工具定义的流式请求。
#[async_trait]
pub trait ModelProvider: Send + Sync {
    async fn stream_chat(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<ToolDefinition>,
    ) -> Result<ChatStream, ProviderError>;
}

/// OpenAI 兼容的 `/chat/completions` 流式客户端。
pub struct OpenAiProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
}

impl OpenAiProvider {
    pub fn new(config: &ModelConfig) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: config.base_url.trim_end_matches('/').to_owned(),
            api_key: config.api_key.clone(),
            model: config.model_name.clone(),
        }
    }

    pub fn model_name(&self) -> &str {
        &self.model
    }
}

#[async_trait]
impl ModelProvider for OpenAiProvider {
    async fn stream_chat(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<ToolDefinition>,
    ) -> Result<ChatStream, ProviderError> {
        let mut body = serde_json::json!({
            "model": self.model,
            "messages": messages.iter().map(ChatMessage::to_json).collect::<Vec<_>>(),
            "stream": true,
        });
        if !tools.is_empty() {
            body["tools"] =
                serde_json::Value::Array(tools.iter().map(ToolDefinition::to_json).collect());
        }
        let url = format!("{}/chat/completions", self.base_url);
        let response = self
            .client
            .post(url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|error| ProviderError::Request(error.to_string()))?;
        let status = response.status();
        if !status.is_success() {
            let retry_after = retry_after(&response);
            let message = response
                .text()
                .await
                .unwrap_or_else(|error| error.to_string());
            return Err(ProviderError::Status {
                status: status.as_u16(),
                message,
                retry_after,
            });
        }
        Ok(parse_stream(response))
    }
}

/// 读取 `Retry-After` 头的秒数形式。HTTP 日期形式不被支持，当作没有建议。
fn retry_after(response: &reqwest::Response) -> Option<Duration> {
    let value = response
        .headers()
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?;
    value.trim().parse::<u64>().ok().map(Duration::from_secs)
}

/// 把 SSE 响应体解析为增量与工具调用片段。
fn parse_stream(response: reqwest::Response) -> ChatStream {
    Box::pin(async_stream::try_stream! {
        let mut buffer: Vec<u8> = Vec::new();
        let mut body = response.bytes_stream();
        while let Some(chunk) = body.next().await {
            let bytes = chunk.map_err(|error| ProviderError::Request(error.to_string()))?;
            buffer.extend_from_slice(&bytes);
            while let Some(position) = buffer.iter().position(|byte| *byte == b'\n') {
                let line: Vec<u8> = buffer.drain(..=position).collect();
                let line = String::from_utf8(line)
                    .map_err(|error| ProviderError::Decode(error.to_string()))?;
                let Some(data) = line.trim_end_matches(['\r', '\n']).strip_prefix("data:") else {
                    continue;
                };
                let data = data.trim();
                if data.is_empty() {
                    continue;
                }
                if data == "[DONE]" {
                    return;
                }
                let value: serde_json::Value = serde_json::from_str(data)
                    .map_err(|error| ProviderError::Decode(format!("{data}: {error}")))?;
                for chunk in chunks_from_value(&value)? {
                    yield chunk;
                }
            }
        }
    })
}

fn chunks_from_value(value: &serde_json::Value) -> Result<Vec<ChatChunk>, ProviderError> {
    if let Some(error) = value.get("error") {
        let message = error
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_else(|| error.as_str().unwrap_or("未知错误"));
        return Err(ProviderError::Upstream(message.to_owned()));
    }
    let Some(delta) = value
        .get("choices")
        .and_then(|choices| choices.get(0))
        .and_then(|choice| choice.get("delta"))
    else {
        return Ok(Vec::new());
    };
    let mut chunks = Vec::new();
    if let Some(content) = delta.get("content").and_then(serde_json::Value::as_str)
        && !content.is_empty()
    {
        chunks.push(ChatChunk::Text(content.to_owned()));
    }
    if let Some(calls) = delta
        .get("tool_calls")
        .and_then(serde_json::Value::as_array)
    {
        for call in calls {
            let index = call
                .get("index")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0) as usize;
            let id = call
                .get("id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned);
            let function = call.get("function");
            let name = function
                .and_then(|function| function.get("name"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned);
            let arguments = function
                .and_then(|function| function.get("arguments"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .to_owned();
            chunks.push(ChatChunk::ToolCall {
                index,
                id,
                name,
                arguments,
            });
        }
    }
    Ok(chunks)
}
