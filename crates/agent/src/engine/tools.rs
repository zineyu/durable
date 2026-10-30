//! 工具循环：先让 tool 节点入树，再执行，再写入结局。批次中途取消时，
//! 剩余调用以「结果未知」入树，保持 assistant 的调用与 tool 节点配对。
//! 工具经 link 交给 executor；契约在 `crate::link`，下一步判定在 `plan`。

use durable::{
    Cancellation, MessageId, RunFailure, RunFailureCode, RunId, RunStart, SessionId, StoreError,
    ToolResultState,
};

use crate::engine::plan::PendingTool;
use crate::engine::{RunEngine, storage_failure};
use crate::link::{ToolCall, ToolContext};

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
        pending: Vec<PendingTool>,
    ) -> Result<ToolLoopEnd, RunFailure> {
        let session = &start.run.session_id;
        let context = ToolContext {
            host: start.host.clone(),
            workspace: start.workspace.clone(),
            cancellation: cancellation.clone(),
        };
        let mut remaining = pending.into_iter();
        while let Some(tool) = remaining.next() {
            let message_id = self
                .ensure_tool_node(session, start.run.id, &tool)
                .await
                .map_err(|error| storage_failure(&error))?;
            let call = ToolCall {
                id: tool.call_id,
                name: tool.name,
                arguments: tool.arguments,
            };
            tracing::debug!(session = %session.as_str(), tool = %call.name, "执行工具");
            let outcome = match self.tools.execute(&call, &context).await {
                Ok(outcome) => outcome,
                Err(closed) => {
                    tracing::warn!(
                        session = %session.as_str(),
                        tool = %call.name,
                        error = %closed,
                        "link 已断开，本次调用结局未知"
                    );
                    self.store
                        .finish_tool(session, message_id, &call.id, ToolResultState::Unknown)
                        .await
                        .map_err(|error| storage_failure(&error))?;
                    self.abandon_tools(session, start.run.id, remaining.collect())
                        .await
                        .map_err(|error| storage_failure(&error))?;
                    return Err(RunFailure {
                        code: RunFailureCode::HostUnreachable,
                        message: closed.to_string(),
                    });
                }
            };
            let state = ToolResultState::from(outcome);
            log_outcome(session, &call.name, &state);
            self.store
                .finish_tool(session, message_id, &call.id, state)
                .await
                .map_err(|error| storage_failure(&error))?;
            if cancellation.is_cancelled() {
                self.abandon_tools(session, start.run.id, remaining.collect())
                    .await
                    .map_err(|error| storage_failure(&error))?;
                return Ok(ToolLoopEnd::Cancelled);
            }
        }
        Ok(ToolLoopEnd::Completed)
    }

    /// 保证该调用有一个 tool 节点，返回节点 id。
    async fn ensure_tool_node(
        &self,
        session: &SessionId,
        run_id: RunId,
        tool: &PendingTool,
    ) -> Result<MessageId, StoreError> {
        if let Some(id) = tool.message {
            return Ok(id);
        }
        let id = self.store.mint_message_id().await?;
        self.store
            .append_tool(session, run_id, id, &tool.call_id)
            .await?;
        Ok(id)
    }

    /// 取消后把没来得及执行的调用写成「结果未知」，避免 assistant 声明了调用却没有对应结果。
    async fn abandon_tools(
        &self,
        session: &SessionId,
        run_id: RunId,
        pending: Vec<PendingTool>,
    ) -> Result<(), StoreError> {
        for tool in pending {
            let message_id = self.ensure_tool_node(session, run_id, &tool).await?;
            self.store
                .finish_tool(session, message_id, &tool.call_id, ToolResultState::Unknown)
                .await?;
        }
        Ok(())
    }
}

fn log_outcome(session: &SessionId, name: &str, state: &ToolResultState) {
    match state {
        ToolResultState::Finished {
            outcome: durable::ToolOutcome::Failed { code, message },
        } => tracing::warn!(
            session = %session.as_str(),
            tool = name,
            code = ?code,
            message = %message,
            "工具失败"
        ),
        ToolResultState::Finished {
            outcome: durable::ToolOutcome::Succeeded { .. },
        } => {
            tracing::debug!(session = %session.as_str(), tool = name, "工具成功");
        }
        ToolResultState::Running | ToolResultState::Unknown => {
            tracing::warn!(session = %session.as_str(), tool = name, "工具结局未知");
        }
    }
}
