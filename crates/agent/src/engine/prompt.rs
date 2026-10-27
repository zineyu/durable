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
    let mut index = 0;
    while let Some(rest) = transcript.get(index..) {
        let Some(node) = rest.first() else {
            break;
        };
        match node.role {
            MessageRole::User => {
                messages.push(ChatMessage::text(ChatRole::User, node.text()));
                index += 1;
            }
            MessageRole::Compaction => {
                messages.push(ChatMessage::text(ChatRole::System, node.text()));
                index += 1;
            }
            MessageRole::Assistant => {
                let (merged, consumed) = merged_assistant(rest);
                messages.push(merged);
                index += consumed;
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
                index += 1;
            }
        }
    }
    messages
}

/// 把从 `rest` 起相邻的 assistant 节点合并成一条 wire 消息，返回消息与被并入的节点数。
///
/// 中断前的部分输出与续跑产出在同一条回答里：文本直接拼接，不插入模型没有写过的分隔符。
fn merged_assistant(rest: &[MessageNode]) -> (ChatMessage, usize) {
    let mut content = String::new();
    let mut tool_calls = Vec::new();
    let mut consumed = 0;
    for node in rest {
        if node.role != MessageRole::Assistant {
            break;
        }
        content.push_str(&node.text());
        tool_calls.extend(
            node.tool_calls()
                .map(|(call_id, name, arguments)| ChatToolCall {
                    id: call_id.to_owned(),
                    name: name.to_owned(),
                    arguments: arguments.to_owned(),
                }),
        );
        consumed += 1;
    }
    (
        ChatMessage {
            role: ChatRole::Assistant,
            content,
            tool_calls,
            tool_call_id: None,
        },
        consumed,
    )
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

#[cfg(test)]
mod tests {
    use super::*;
    use durable::{ContentPart, MessageId};

    fn node(id: i64, role: MessageRole, parts: Vec<ContentPart>) -> MessageNode {
        MessageNode {
            id: MessageId::parse(id).expect("合法消息 id"),
            parent_id: None,
            role,
            interrupted: false,
            parts,
            run_id: None,
            created_at: id,
        }
    }

    fn assistant(id: i64, text: &str) -> MessageNode {
        node(id, MessageRole::Assistant, vec![ContentPart::text(text)])
    }

    fn interrupted(id: i64, text: &str) -> MessageNode {
        let mut message = assistant(id, text);
        message.interrupted = true;
        message
    }

    #[test]
    fn adjacent_assistant_messages_merge_into_one() {
        let transcript = [
            node(1, MessageRole::User, vec![ContentPart::text("做点事")]),
            interrupted(2, "已经写了一半"),
            assistant(3, "，续写完成。"),
        ];

        let messages = build_messages(&transcript);

        assert_eq!(
            messages.len(),
            3,
            "系统提示、用户消息与合并后的一条 assistant"
        );
        assert_eq!(messages[2].role, ChatRole::Assistant);
        assert_eq!(
            messages[2].content, "已经写了一半，续写完成。",
            "相邻的 assistant 文本直接拼接，不插入它没有写过的文字"
        );
    }

    #[test]
    fn tool_results_keep_assistant_messages_apart() {
        let call = node(
            2,
            MessageRole::Assistant,
            vec![ContentPart::tool_call("call_1", "read_file", "{}")],
        );
        let result = node(
            3,
            MessageRole::Tool,
            vec![ContentPart::tool_result("call_1", ToolResultState::Unknown)],
        );
        let transcript = [
            node(1, MessageRole::User, vec![ContentPart::text("做点事")]),
            call,
            result,
            assistant(4, "读到了。"),
        ];

        let messages = build_messages(&transcript);

        assert_eq!(messages.len(), 5, "工具结果两侧的 assistant 消息不合并");
        assert_eq!(messages[3].role, ChatRole::Tool);
        assert_eq!(messages[4].content, "读到了。");
    }
}
