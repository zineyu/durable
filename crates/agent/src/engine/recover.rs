//! 续跑点推导：从树尾判断本轮该发请求、执行工具还是收尾。
//! 执行循环在父模块，工具执行在 `tools`。

use std::collections::HashMap;

use durable::{MessageId, MessageNode, MessageRole, ToolResultState};

/// 下一步该做的事。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ResumePoint {
    /// 发一次模型请求。
    Request,
    /// 执行这些工具调用。
    Tools(Vec<PendingTool>),
    /// 最后一条 assistant 消息没有请求工具，run 正常收尾。
    Finish,
}

/// 待执行的工具调用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PendingTool {
    pub call_id: String,
    pub name: String,
    pub arguments: String,
    /// 已在树中的执行中节点；`None` 表示还没入树。
    pub message: Option<MessageId>,
}

/// 从 transcript 的树尾推导下一步。
///
/// 末条是用户或压缩消息时发请求；末条是 assistant 且没有工具调用时收尾；
/// 否则按最近一条 assistant 消息的调用列表找出尚未完成的工具：缺节点的补入树，
/// 执行中的节点重跑。工具批次中途崩溃因此不会丢掉剩余调用。
pub(super) fn resume_point(transcript: &[MessageNode]) -> ResumePoint {
    let Some(last) = transcript.last() else {
        return ResumePoint::Request;
    };
    let assistant = match last.role {
        MessageRole::User | MessageRole::Compaction => return ResumePoint::Request,
        MessageRole::Assistant => Some(last),
        MessageRole::Tool => transcript
            .iter()
            .rev()
            .find(|node| node.role == MessageRole::Assistant),
    };
    let Some(assistant) = assistant else {
        return ResumePoint::Request;
    };
    if assistant.tool_calls().next().is_none() {
        return ResumePoint::Finish;
    }
    let results = tool_results(transcript);
    let pending: Vec<PendingTool> = assistant
        .tool_calls()
        .filter_map(|(call_id, name, arguments)| match results.get(call_id) {
            Some(&(_, ToolResultState::Finished { .. } | ToolResultState::Unknown)) => None,
            Some(&(message, ToolResultState::Running)) => Some(PendingTool {
                call_id: call_id.to_owned(),
                name: name.to_owned(),
                arguments: arguments.to_owned(),
                message: Some(message),
            }),
            None => Some(PendingTool {
                call_id: call_id.to_owned(),
                name: name.to_owned(),
                arguments: arguments.to_owned(),
                message: None,
            }),
        })
        .collect();
    if pending.is_empty() {
        ResumePoint::Request
    } else {
        ResumePoint::Tools(pending)
    }
}

fn tool_results(transcript: &[MessageNode]) -> HashMap<&str, (MessageId, &ToolResultState)> {
    transcript
        .iter()
        .flat_map(|node| {
            node.tool_results()
                .map(move |(call_id, state)| (call_id, (node.id, state)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use durable::ContentPart;

    fn node(
        id: i64,
        parent: Option<i64>,
        role: MessageRole,
        parts: Vec<ContentPart>,
    ) -> MessageNode {
        MessageNode {
            id: MessageId::parse(id).expect("合法消息 id"),
            parent_id: parent.map(|id| MessageId::parse(id).expect("合法消息 id")),
            role,
            parts,
            run_id: None,
            created_at: id,
        }
    }

    fn user(id: i64) -> MessageNode {
        node(
            id,
            None,
            MessageRole::User,
            vec![ContentPart::text("做点事")],
        )
    }

    fn assistant(id: i64, parent: i64, calls: &[&str]) -> MessageNode {
        let parts = calls
            .iter()
            .map(|call_id| ContentPart::tool_call(*call_id, "read_file", "{}"))
            .collect();
        node(id, Some(parent), MessageRole::Assistant, parts)
    }

    fn tool(id: i64, parent: i64, call_id: &str, state: ToolResultState) -> MessageNode {
        node(
            id,
            Some(parent),
            MessageRole::Tool,
            vec![ContentPart::tool_result(call_id, state)],
        )
    }

    fn finished() -> ToolResultState {
        ToolResultState::Finished {
            outcome: durable::ToolOutcome::Succeeded {
                content: "好".to_owned(),
            },
        }
    }

    #[test]
    fn empty_transcript_requests_a_step() {
        assert_eq!(resume_point(&[]), ResumePoint::Request);
    }

    #[test]
    fn trailing_user_requests_a_step() {
        assert_eq!(resume_point(&[user(1)]), ResumePoint::Request);
    }

    #[test]
    fn trailing_assistant_without_calls_finishes() {
        let transcript = [
            user(1),
            node(
                2,
                Some(1),
                MessageRole::Assistant,
                vec![ContentPart::text("答")],
            ),
        ];
        assert_eq!(resume_point(&transcript), ResumePoint::Finish);
    }

    #[test]
    fn trailing_assistant_with_calls_runs_all_of_them() {
        let transcript = [user(1), assistant(2, 1, &["call_a", "call_b"])];
        let ResumePoint::Tools(pending) = resume_point(&transcript) else {
            panic!("应执行工具");
        };
        assert_eq!(pending.len(), 2);
        assert!(pending.iter().all(|tool| tool.message.is_none()));
    }

    #[test]
    fn partial_tool_batch_runs_only_the_missing_calls() {
        let transcript = [
            user(1),
            assistant(2, 1, &["call_a", "call_b"]),
            tool(3, 2, "call_a", finished()),
        ];
        let ResumePoint::Tools(pending) = resume_point(&transcript) else {
            panic!("应补齐剩余工具");
        };
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].call_id, "call_b");
        assert_eq!(pending[0].message, None);
    }

    #[test]
    fn running_tool_is_rerun_on_its_own_node() {
        let transcript = [
            user(1),
            assistant(2, 1, &["call_a"]),
            tool(3, 2, "call_a", ToolResultState::Running),
        ];
        let ResumePoint::Tools(pending) = resume_point(&transcript) else {
            panic!("应重跑执行中的工具");
        };
        assert_eq!(
            pending[0].message,
            Some(MessageId::parse(3).expect("合法消息 id"))
        );
    }

    #[test]
    fn completed_tool_batch_requests_the_next_step() {
        let transcript = [
            user(1),
            assistant(2, 1, &["call_a"]),
            tool(3, 2, "call_a", finished()),
        ];
        assert_eq!(resume_point(&transcript), ResumePoint::Request);
    }

    #[test]
    fn unknown_outcome_counts_as_completed() {
        let transcript = [
            user(1),
            assistant(2, 1, &["call_a"]),
            tool(3, 2, "call_a", ToolResultState::Unknown),
        ];
        assert_eq!(resume_point(&transcript), ResumePoint::Request);
    }
}
