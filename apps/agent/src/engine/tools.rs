//! 工具循环：先让 tool 节点入树（已登记未下发），下发前写一次已下发，再执行，再写入结局。
//! 批次中途取消时，剩余调用以「结果未知」入树，保持 assistant 的调用与 tool 节点配对。
//! 工具经 link 交给 executor；契约在 `crate::link`，下一步判定在 `plan`。
//!
//! 三个提交点由许可证串起来：没有已下发的许可证就调不到执行端（`effects::token`）。
//! 宿主丢失只有一处翻译：在途调用写「结果未知」，剩余调用同样写「结果未知」，
//! run 以 `host_unreachable` 结束。

use std::time::Instant;

use durable::{
    Cancellation, EffectClass, EffectIntent, EffectObservation, MessageId, RunFailure,
    RunFailureCode, RunId, RunStart, SessionId, StoreError, ToolCall, ToolResultState,
};
use effects::token::{Abandon, Lost, Outcome, abandon, declare, dispatch, perform};

use crate::engine::plan::PendingCall;
use crate::engine::{RunEngine, elapsed_ms, storage_failure};
use crate::link::{ToolContext, ToolEnd};

/// 工具循环的结束方式。
///
/// 取消单独成一支：它既不是成功也不是失败，由调用方翻成 run 结局。
pub(super) enum ToolLoopEnd {
    /// 批次执行完毕。
    Completed,
    /// 执行中被取消，剩余调用已写成「结果未知」。
    Cancelled,
}

impl RunEngine {
    /// 顺序执行待办工具调用：缺节点的先入树，执行完把结局写回同一节点。
    ///
    /// 取消时剩余调用写成「结果未知」，并由调用方结束 run。
    pub(super) async fn run_tool_calls(
        &self,
        start: &RunStart,
        cancellation: &Cancellation,
        pending: Vec<PendingCall>,
    ) -> Result<ToolLoopEnd, RunFailure> {
        let session = &start.run.session_id;
        let context = ToolContext {
            host: start.host.clone(),
            workspace: start.workspace.clone(),
            cancellation: cancellation.clone(),
        };
        let mut remaining = pending.into_iter();
        while let Some(pending) = remaining.next() {
            let call = pending.call;
            let message_id = self.node_of(pending.node).await?;
            let declared = declare(
                self.store.as_ref(),
                intent_of(&call, session, start.run.id, message_id),
            )
            .await
            .map_err(|error| storage_failure(&error))?;
            // 可恢复状态先落库，再执行（ADR 0037）。
            let dispatched = dispatch(self.store.as_ref(), declared)
                .await
                .map_err(|error| storage_failure(&error))?;
            tracing::debug!(session = %session.as_str(), tool = %call.name, "执行工具");
            let started = Instant::now();
            let outcome = perform(self.store.as_ref(), dispatched, |token| {
                // 借用与许可证分开取：工作体拿走许可证，调用与上下文仍归循环所有。
                let call = &call;
                let context = &context;
                async move {
                    match self.tools.execute(call, context, &token).await {
                        ToolEnd::Ended(outcome) => {
                            log_outcome(session, &call.name, &outcome);
                            Outcome::Seen(EffectObservation::Tool(ToolResultState::Finished {
                                outcome,
                                duration_ms: elapsed_ms(started.elapsed()),
                            }))
                        }
                        ToolEnd::HostGone(closed) => {
                            Outcome::Lost(Lost::HostGone(closed.to_string()))
                        }
                    }
                }
            })
            .await
            .map_err(|error| storage_failure(&error))?;
            // 工作体只在宿主丢失时给出 `Lost`：本循环没有别的介质。在途调用的结局已由
            // 落点表写成「结果未知」，因此这里只剩翻译成 run 失败一件事。
            if let Outcome::Lost(lost) = outcome {
                tracing::warn!(
                    session = %session.as_str(),
                    tool = %call.name,
                    reason = %lost,
                    "link 已断开，本次调用的结局未知"
                );
                self.abandon_tools(
                    session,
                    start.run.id,
                    remaining.collect(),
                    Abandon::HostGone(lost.to_string()),
                )
                .await
                .map_err(|error| storage_failure(&error))?;
                return Err(RunFailure {
                    code: RunFailureCode::HostUnreachable,
                    message: lost.to_string(),
                });
            }
            if cancellation.is_cancelled() {
                self.abandon_tools(
                    session,
                    start.run.id,
                    remaining.collect(),
                    Abandon::Cancelled,
                )
                .await
                .map_err(|error| storage_failure(&error))?;
                return Ok(ToolLoopEnd::Cancelled);
            }
        }
        Ok(ToolLoopEnd::Completed)
    }

    /// 本次调用的 tool 节点：已有节点直接复用，否则分配一个新的。
    ///
    /// 新建节点的写入在「登记」那一步，因此这里只分配 id。
    async fn node_of(&self, node: Option<MessageId>) -> Result<MessageId, RunFailure> {
        if let Some(node) = node {
            return Ok(node);
        }
        self.store
            .mint_message_id()
            .await
            .map_err(|error| storage_failure(&error))
    }

    /// 取消或断链后把没来得及执行的调用写成「结果未知」，避免 assistant 声明了调用却没有对应结果。
    ///
    /// 这些调用已登记但一定没有执行：`abandon` 正是这一支。
    async fn abandon_tools(
        &self,
        session: &SessionId,
        run_id: RunId,
        pending: Vec<PendingCall>,
        reason: Abandon,
    ) -> Result<(), StoreError> {
        for pending in pending {
            let message_id = match pending.node {
                Some(node) => node,
                None => self.store.mint_message_id().await?,
            };
            let intent = intent_of(&pending.call, session, run_id, message_id);
            let declared = declare(self.store.as_ref(), intent).await?;
            let _outcome = abandon(self.store.as_ref(), declared, reason.clone()).await?;
        }
        Ok(())
    }
}

/// 一次工具调用的意图。未登记的工具名没有类别：它一定没有副作用，重放按 `Redo`。
fn intent_of(call: &ToolCall, session: &SessionId, run: RunId, message: MessageId) -> EffectIntent {
    match EffectClass::of_tool(&call.name) {
        Some(class) => EffectIntent::tool(class, session.clone(), run, message, call.id.clone()),
        None => EffectIntent::unregistered_tool(session.clone(), run, message, call.id.clone()),
    }
}

/// 日志记录一次工具结局。只收结局：工具循环里能走到的只有成功与失败两支，
/// 「结果未知」在宿主丢失的分支里单独记录。
fn log_outcome(session: &SessionId, name: &str, outcome: &durable::ToolOutcome) {
    match outcome {
        durable::ToolOutcome::Failed { code, message } => tracing::warn!(
            session = %session.as_str(),
            tool = name,
            code = ?code,
            message = %message,
            "工具失败"
        ),
        durable::ToolOutcome::Succeeded { .. } => {
            tracing::debug!(session = %session.as_str(), tool = name, "工具成功");
        }
    }
}
