//! 把 transcript 装配成模型请求：系统提示、工具结果回填与上下文长度。
//! 请求本身与流式解析在 `crate::provider`。

use durable::{MessageNode, MessageRole, RunFailure, RunFailureCode, SessionId, ToolOutcome};

use crate::engine::RunEngine;
use crate::provider::{ChatMessage, ChatRole, ChatToolCall};

const SYSTEM_PROMPT: &str = "你是运行在本机 workspace 根目录中的 coding agent。\
使用提供的工具读取文件、写入文件、精确编辑文件与执行命令；所有路径都必须位于 workspace 根内。\
禁止猜测文件内容，需要时先读取文件。回答保持简洁。";

fn build_messages(transcript: &[MessageNode]) -> Vec<ChatMessage> {
    let mut messages = vec![ChatMessage::text(ChatRole::System, SYSTEM_PROMPT)];
    for node in transcript {
        match node.role {
            MessageRole::User => {
                messages.push(ChatMessage::text(ChatRole::User, node.text.as_str()));
            }
            MessageRole::Assistant => {
                messages.push(ChatMessage {
                    role: ChatRole::Assistant,
                    content: node.text.as_str().to_owned(),
                    tool_calls: node
                        .tool_calls
                        .iter()
                        .map(|record| ChatToolCall {
                            id: record.id.clone(),
                            name: record.name.clone(),
                            arguments: record.arguments.clone(),
                        })
                        .collect(),
                    tool_call_id: None,
                });
                for record in &node.tool_calls {
                    messages.push(ChatMessage {
                        role: ChatRole::Tool,
                        content: tool_result_content(&record.outcome),
                        tool_calls: Vec::new(),
                        tool_call_id: Some(record.id.clone()),
                    });
                }
            }
        }
    }
    messages
}

fn tool_result_content(outcome: &ToolOutcome) -> String {
    match outcome {
        ToolOutcome::Succeeded { content } => content.clone(),
        ToolOutcome::Failed { code, message } => {
            serde_json::json!({ "error": code, "message": message }).to_string()
        }
    }
}

fn char_count(messages: &[ChatMessage]) -> usize {
    messages
        .iter()
        .map(|message| {
            message.content.chars().count()
                + message
                    .tool_calls
                    .iter()
                    .map(|call| call.name.chars().count() + call.arguments.chars().count())
                    .sum::<usize>()
        })
        .sum()
}

impl RunEngine {
    /// 读取当前 transcript 并装配成本轮请求。
    ///
    /// 返回 `Err` 时携带终止 run 的原因：存储读取失败或上下文超限。
    pub(super) async fn step_messages(
        &self,
        session: &SessionId,
    ) -> Result<Vec<ChatMessage>, RunFailure> {
        let transcript = match self.store.transcript(session).await {
            Ok(transcript) => transcript,
            Err(error) => {
                return Err(RunFailure {
                    code: RunFailureCode::ModelUpstream,
                    message: error.to_string(),
                });
            }
        };
        let messages = build_messages(&transcript);
        if char_count(&messages) > self.config.context_char_limit {
            return Err(RunFailure {
                code: RunFailureCode::ContextLimitExceeded,
                message: format!("上下文超过 {} 字符上限", self.config.context_char_limit),
            });
        }
        Ok(messages)
    }
}
