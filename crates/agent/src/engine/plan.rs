//! 「下一步做什么」的判定：从 transcript 与当前 run 推出 run 动作。
//! 执行循环在父模块，模型流在 `step`，工具执行在 `tools`，压缩在 `compact`。
//!
//! 前提：判定只看当前 transcript。链在压缩节点处截断，因此压缩之前的消息不参与判定，
//! 包括步数计数。

use std::collections::HashMap;

use durable::{MessageId, MessageNode, MessageRole, RunId, ToolCall, ToolResultState};

/// 下一步动作。
#[derive(Debug, PartialEq)]
pub(super) enum NextAction {
    /// 发一次模型请求。
    Request,
    /// 执行这些工具调用。
    Tools(Vec<PendingCall>),
    /// 最后一条 assistant 消息没有请求工具，run 正常收尾。
    Finish,
    /// 当前 run 已达到步数上限。
    StepLimit,
}

/// 一次判定的结果：下一步动作，以及此刻是否允许接入待插入插话。
///
/// 压缩不在动作里：压缩是对模型请求失败的回应，无法从 transcript 推出，
/// 因此它留在执行循环的失败分支。
pub(super) struct Plan {
    /// 允许接入插话。工具执行期间与步数已达上限时不允许。
    pub steer: bool,
    pub action: NextAction,
}

/// 待执行的工具调用：调用身份，以及它在树中已有的节点。
#[derive(Debug, PartialEq)]
pub(super) struct PendingCall {
    pub call: ToolCall,
    /// 已在树中的执行中节点；`None` 表示还没入树。
    pub node: Option<MessageId>,
}

/// 从 transcript 与当前 run 推出下一步。
///
/// 步数上限先于请求判定，且只挡请求：限额用尽时仍执行未完成的工具批，
/// 插话留到下一次 run。工具执行期间不接入插话，正在执行的进程因此不被打断。
///
/// 步数是当前 transcript 里属于该 run 的 assistant 节点数。链在压缩节点处截断，
/// 因此压缩之后计数从零起算，`max_steps` 不是请求次数的硬上限（ADR 0049）。
pub(super) fn next_action(transcript: &[MessageNode], run: RunId, max_steps: usize) -> Plan {
    let action = action_of(transcript);
    if matches!(action, NextAction::Request) && requests_started(transcript, run) >= max_steps {
        return Plan {
            steer: false,
            action: NextAction::StepLimit,
        };
    }
    Plan {
        steer: !matches!(action, NextAction::Tools(_)),
        action,
    }
}

/// 已走过的步数：当前 transcript 里属于该 run 的 assistant 节点数。
///
/// 压缩节点把它们截在链外，因此压缩之后从零起算。这是有意接受的代价，见 ADR 0049。
fn requests_started(transcript: &[MessageNode], run: RunId) -> usize {
    transcript
        .iter()
        .filter(|node| node.role == MessageRole::Assistant && node.run_id == Some(run))
        .count()
}

/// 从 transcript 的树尾推导下一步动作，不含步数上限。
///
/// 末条是用户或压缩消息时发请求；末条是已中断的 assistant 消息时发请求，让模型接着
/// 自己写过的部分继续；末条是 assistant 且没有工具调用时收尾；否则按最近一条 assistant
/// 消息的调用列表找出尚未完成的工具：缺节点的补入树，执行中的节点重跑。
/// 工具批次中途崩溃因此不会丢掉剩余调用。
fn action_of(transcript: &[MessageNode]) -> NextAction {
    let Some(last) = transcript.last() else {
        return NextAction::Request;
    };
    let assistant = match last.role {
        MessageRole::User | MessageRole::Compaction => return NextAction::Request,
        MessageRole::Assistant if last.interrupted => return NextAction::Request,
        MessageRole::Assistant => Some(last),
        MessageRole::Tool => transcript
            .iter()
            .rev()
            .find(|node| node.role == MessageRole::Assistant),
    };
    let Some(assistant) = assistant else {
        return NextAction::Request;
    };
    if assistant.tool_calls().next().is_none() {
        return NextAction::Finish;
    }
    let results = tool_results(transcript);
    let pending: Vec<PendingCall> = assistant
        .tool_calls()
        .filter_map(|(call_id, name, arguments)| {
            let node = match results.get(call_id) {
                Some(&(_, ToolResultState::Finished { .. } | ToolResultState::Unknown)) => {
                    return None;
                }
                Some(&(message, ToolResultState::Running)) => Some(message),
                None => None,
            };
            Some(PendingCall {
                call: ToolCall {
                    id: call_id.to_owned(),
                    name: name.to_owned(),
                    arguments: arguments.to_owned(),
                },
                node,
            })
        })
        .collect();
    if pending.is_empty() {
        NextAction::Request
    } else {
        NextAction::Tools(pending)
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
    use durable::{ContentPart, MessageId, MessageNode, MessageRole, RunId, ToolResultState};

    fn current_run() -> RunId {
        RunId::parse(1).expect("合法 run id")
    }

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
            interrupted: false,
            parts,
            run_id: None,
            created_at: id,
        }
    }

    fn user(id: i64) -> MessageNode {
        MessageNode {
            run_id: Some(current_run()),
            ..node(
                id,
                None,
                MessageRole::User,
                vec![ContentPart::text("做点事")],
            )
        }
    }

    fn assistant(id: i64, parent: i64, calls: &[&str], run: Option<RunId>) -> MessageNode {
        let parts = calls
            .iter()
            .map(|call_id| ContentPart::tool_call(*call_id, "read_file", "{}"))
            .collect();
        MessageNode {
            run_id: run,
            ..node(id, Some(parent), MessageRole::Assistant, parts)
        }
    }

    fn assistant_of_current_run(id: i64, parent: i64, calls: &[&str]) -> MessageNode {
        assistant(id, parent, calls, Some(current_run()))
    }

    fn reply(id: i64, parent: i64) -> MessageNode {
        MessageNode {
            run_id: Some(current_run()),
            ..node(
                id,
                Some(parent),
                MessageRole::Assistant,
                vec![ContentPart::text("答")],
            )
        }
    }

    fn tool(id: i64, parent: i64, call_id: &str, state: ToolResultState) -> MessageNode {
        node(
            id,
            Some(parent),
            MessageRole::Tool,
            vec![ContentPart::tool_result(call_id, state)],
        )
    }

    fn compaction(id: i64, parent: Option<i64>) -> MessageNode {
        node(
            id,
            parent,
            MessageRole::Compaction,
            vec![ContentPart::text("摘要")],
        )
    }

    fn finished() -> ToolResultState {
        ToolResultState::Finished {
            outcome: durable::ToolOutcome::Succeeded {
                content: "好".to_owned(),
            },
        }
    }

    fn plan(transcript: &[MessageNode], max_steps: usize) -> super::Plan {
        super::next_action(transcript, current_run(), max_steps)
    }

    #[test]
    fn empty_transcript_requests_a_step() {
        assert_eq!(plan(&[], usize::MAX).action, super::NextAction::Request);
    }

    #[test]
    fn trailing_user_requests_a_step() {
        assert_eq!(
            plan(&[user(1)], usize::MAX).action,
            super::NextAction::Request
        );
    }

    #[test]
    fn interrupted_assistant_requests_the_next_step() {
        let mut interrupted = reply(2, 1);
        interrupted.interrupted = true;
        let transcript = [user(1), interrupted];
        assert_eq!(
            plan(&transcript, usize::MAX).action,
            super::NextAction::Request
        );
    }

    #[test]
    fn trailing_assistant_without_calls_finishes() {
        let transcript = [user(1), reply(2, 1)];
        assert_eq!(
            plan(&transcript, usize::MAX).action,
            super::NextAction::Finish
        );
    }

    #[test]
    fn trailing_assistant_with_calls_runs_all_of_them() {
        let transcript = [
            user(1),
            assistant_of_current_run(2, 1, &["call_a", "call_b"]),
        ];
        let super::NextAction::Tools(pending) = plan(&transcript, usize::MAX).action else {
            panic!("应执行工具");
        };
        assert_eq!(pending.len(), 2);
        assert!(pending.iter().all(|pending| pending.node.is_none()));
    }

    #[test]
    fn partial_tool_batch_runs_only_the_missing_calls() {
        let transcript = [
            user(1),
            assistant_of_current_run(2, 1, &["call_a", "call_b"]),
            tool(3, 2, "call_a", finished()),
        ];
        let super::NextAction::Tools(pending) = plan(&transcript, usize::MAX).action else {
            panic!("应补齐剩余工具");
        };
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].call.id, "call_b");
        assert_eq!(pending[0].node, None);
    }

    #[test]
    fn running_tool_is_rerun_on_its_own_node() {
        let transcript = [
            user(1),
            assistant_of_current_run(2, 1, &["call_a"]),
            tool(3, 2, "call_a", ToolResultState::Running),
        ];
        let super::NextAction::Tools(pending) = plan(&transcript, usize::MAX).action else {
            panic!("应重跑执行中的工具");
        };
        assert_eq!(
            pending[0].node,
            Some(MessageId::parse(3).expect("合法消息 id"))
        );
    }

    #[test]
    fn completed_tool_batch_requests_the_next_step() {
        let transcript = [
            user(1),
            assistant_of_current_run(2, 1, &["call_a"]),
            tool(3, 2, "call_a", finished()),
        ];
        assert_eq!(
            plan(&transcript, usize::MAX).action,
            super::NextAction::Request
        );
    }

    #[test]
    fn unknown_outcome_counts_as_completed() {
        let transcript = [
            user(1),
            assistant_of_current_run(2, 1, &["call_a"]),
            tool(3, 2, "call_a", ToolResultState::Unknown),
        ];
        assert_eq!(
            plan(&transcript, usize::MAX).action,
            super::NextAction::Request
        );
    }

    #[test]
    fn step_limit_blocks_the_next_request() {
        let transcript = [
            user(1),
            assistant_of_current_run(2, 1, &["call_a"]),
            tool(3, 2, "call_a", finished()),
        ];
        let plan = plan(&transcript, 1);
        assert_eq!(plan.action, super::NextAction::StepLimit);
        assert!(!plan.steer, "限额用尽时不接入插话");
    }

    #[test]
    fn step_limit_does_not_block_a_tool_batch() {
        let transcript = [user(1), assistant_of_current_run(2, 1, &["call_a"])];
        let plan = plan(&transcript, 1);
        assert!(matches!(plan.action, super::NextAction::Tools(_)));
        assert!(!plan.steer, "工具执行期间不接入插话");
    }

    #[test]
    fn steering_is_allowed_before_finishing() {
        let transcript = [user(1), reply(2, 1)];
        assert!(plan(&transcript, usize::MAX).steer);
    }

    #[test]
    fn steering_is_allowed_before_a_request() {
        let transcript = [user(1)];
        assert!(plan(&transcript, usize::MAX).steer);
    }

    #[test]
    fn compaction_at_the_tail_requests_a_step() {
        let transcript = [user(1), reply(2, 1), compaction(3, Some(2))];
        let plan = plan(&transcript, usize::MAX);
        assert_eq!(plan.action, super::NextAction::Request);
        assert!(plan.steer);
    }

    #[test]
    fn interrupted_assistant_with_calls_requests_the_next_step() {
        let mut interrupted = assistant_of_current_run(2, 1, &["call_a"]);
        interrupted.interrupted = true;
        let transcript = [user(1), interrupted];
        assert_eq!(
            plan(&transcript, usize::MAX).action,
            super::NextAction::Request
        );
    }

    #[test]
    fn steps_before_the_compaction_are_not_counted() {
        // 链在压缩节点处截断，压缩之前的 assistant 节点已不在 transcript 里。
        let transcript = [
            compaction(1, None),
            assistant_of_current_run(3, 1, &["call_a"]),
            tool(4, 3, "call_a", finished()),
        ];
        assert_eq!(plan(&transcript, 1).action, super::NextAction::StepLimit);
        assert_eq!(plan(&transcript, 2).action, super::NextAction::Request);
    }

    #[test]
    fn steps_of_earlier_runs_are_not_counted() {
        let earlier = RunId::parse(9).expect("合法 run id");
        let transcript = [
            user(1),
            assistant(2, 1, &["call_a"], Some(earlier)),
            tool(3, 2, "call_a", finished()),
            user(4),
        ];
        assert_eq!(plan(&transcript, 1).action, super::NextAction::Request);
    }
}
