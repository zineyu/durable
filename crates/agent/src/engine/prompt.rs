//! 把 transcript 装配成模型请求：系统提示、块到 wire 形状与上下文长度。
//! 请求本身与流式解析在 `crate::provider`。

use durable::{MessageNode, MessageRole, RunFailure, RunFailureCode, ToolOutcome, ToolResultState};

use crate::engine::RunEngine;
use crate::provider::{ChatMessage, ChatRole, ChatToolCall};

const SYSTEM_PROMPT: &str = "你是运行在本机 workspace 根目录中的 coding agent。\
使用提供的工具读取文件、写入文件、精确编辑文件与执行命令；所有路径都必须位于 workspace 根内。\
禁止猜测文件内容，需要时先读取文件。回答保持简洁。";

/// 压缩请求的系统提示：要摘要，不要回答对话里的问题。
const COMPACTION_PROMPT: &str = "把下面的对话压缩成一段摘要：保留未完成的事项、已确认的结论、\
涉及的路径与命令，以及仍然有效的工作假设。不要回答对话里的问题，不要添加新内容。只输出摘要正文。";

fn build_messages(transcript: &[MessageNode]) -> Vec<ChatMessage> {
    let mut messages = vec![ChatMessage::text(ChatRole::System, SYSTEM_PROMPT)];
    for node in transcript {
        match node.role {
            MessageRole::User => {
                messages.push(ChatMessage::text(ChatRole::User, node.text()));
            }
            MessageRole::Compaction => {
                messages.push(ChatMessage::text(ChatRole::System, node.text()));
            }
            MessageRole::Assistant => {
                messages.push(ChatMessage {
                    role: ChatRole::Assistant,
                    content: node.text(),
                    tool_calls: node
                        .tool_calls()
                        .map(|(call_id, name, arguments)| ChatToolCall {
                            id: call_id.to_owned(),
                            name: name.to_owned(),
                            arguments: arguments.to_owned(),
                        })
                        .collect(),
                    tool_call_id: None,
                });
            }
            MessageRole::Tool => {
                for (call_id, state) in node.tool_results() {
                    messages.push(ChatMessage {
                        role: ChatRole::Tool,
                        content: tool_result_content(state),
                        tool_calls: Vec::new(),
                        tool_call_id: Some(call_id.to_owned()),
                    });
                }
            }
        }
    }
    messages
}

fn tool_result_content(state: &ToolResultState) -> String {
    match state {
        ToolResultState::Finished {
            outcome: ToolOutcome::Succeeded { content },
        } => content.clone(),
        ToolResultState::Finished {
            outcome: ToolOutcome::Failed { code, message },
        } => serde_json::json!({ "error": code, "message": message }).to_string(),
        ToolResultState::Running => {
            serde_json::json!({ "error": "running", "message": "工具仍在执行" }).to_string()
        }
        ToolResultState::Unknown => {
            serde_json::json!({ "error": "unknown", "message": "结果未知" }).to_string()
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

/// 装配压缩请求：系统提示要摘要，用户消息是待压缩的对话原文。
pub(super) fn compaction_messages(transcript: &[MessageNode]) -> Vec<ChatMessage> {
    vec![
        ChatMessage::text(ChatRole::System, COMPACTION_PROMPT),
        ChatMessage::text(ChatRole::User, render_transcript(transcript)),
    ]
}

/// 把 transcript 渲染成可压缩的纯文本：每条消息一段，带上角色标签。
fn render_transcript(transcript: &[MessageNode]) -> String {
    let mut rendered = String::new();
    for message in build_messages(transcript).iter().skip(1) {
        rendered.push_str(role_label(message.role));
        rendered.push('\n');
        rendered.push_str(&message.content);
        for call in &message.tool_calls {
            rendered.push_str("\n调用 ");
            rendered.push_str(&call.name);
            rendered.push_str(": ");
            rendered.push_str(&call.arguments);
        }
        rendered.push_str("\n\n");
    }
    rendered
}

fn role_label(role: ChatRole) -> &'static str {
    match role {
        ChatRole::System => "【系统】",
        ChatRole::User => "【用户】",
        ChatRole::Assistant => "【assistant】",
        ChatRole::Tool => "【工具结果】",
    }
}

impl RunEngine {
    /// 把当前 transcript 装配成本轮请求，并检查上下文长度。
    ///
    /// 返回 `Err` 时携带终止 run 的原因：上下文超限。
    pub(super) fn assemble(
        &self,
        transcript: &[MessageNode],
    ) -> Result<Vec<ChatMessage>, RunFailure> {
        let messages = build_messages(transcript);
        if char_count(&messages) > self.config.context_char_limit {
            return Err(RunFailure {
                code: RunFailureCode::ContextLimitExceeded,
                message: format!("上下文超过 {} 字符上限", self.config.context_char_limit),
            });
        }
        Ok(messages)
    }
}
