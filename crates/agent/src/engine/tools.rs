//! 工具循环：按序执行本轮工具调用并逐条落库。
//! 工具实现与 workspace 边界在 `crate::tools`。

use durable::{MessageId, RunStart, SessionId, ToolCallRecord, ToolOutcome};

use crate::cancel::Cancellation;
use crate::engine::{RunEngine, ToolCallBuilder};
use crate::tools::{ToolCall, ToolContext};

impl RunEngine {
    /// 顺序执行本轮的每个工具调用，结果写入 `records` 并落库。
    ///
    /// 返回 `true` 表示本轮被取消，调用方应结束 run。
    pub(super) async fn run_tool_calls(
        &self,
        start: &RunStart,
        cancellation: &Cancellation,
        assistant_id: MessageId,
        builders: Vec<ToolCallBuilder>,
        text: &str,
        records: &mut Vec<ToolCallRecord>,
    ) -> bool {
        let session = &start.run.session_id;
        let calls: Vec<ToolCall> = builders
            .into_iter()
            .enumerate()
            .map(|(index, builder)| ToolCall {
                id: builder.id.unwrap_or_else(|| format!("call_{}", index + 1)),
                name: builder.name.unwrap_or_default(),
                arguments: builder.arguments,
            })
            .collect();
        let context = ToolContext {
            workspace: start.workspace.clone(),
            cancellation: cancellation.clone(),
        };
        for call in calls {
            tracing::debug!(session = %session.as_str(), tool = %call.name, "执行工具");
            let outcome = tokio::select! {
                biased;
                () = cancellation.cancelled() => {
                    self.flush(assistant_id, text, records).await;
                    return true;
                }
                outcome = self.tools.execute(&call, &context) => outcome,
            };
            let record = ToolCallRecord {
                id: call.id,
                name: call.name,
                arguments: call.arguments,
                outcome,
            };
            match &record.outcome {
                ToolOutcome::Failed { code, message } => tracing::warn!(
                    session = %session.as_str(),
                    tool = %record.name,
                    code = ?code,
                    message = %message,
                    "工具失败"
                ),
                ToolOutcome::Succeeded { .. } => tracing::debug!(
                    session = %session.as_str(),
                    tool = %record.name,
                    "工具成功"
                ),
            }
            self.record_tool_call(session, assistant_id, &record).await;
            records.push(record);
        }
        false
    }

    pub(super) async fn record_tool_call(
        &self,
        session: &SessionId,
        assistant_id: MessageId,
        call: &ToolCallRecord,
    ) {
        if let Err(error) = self
            .store
            .record_tool_call(session, assistant_id, call.clone())
            .await
        {
            tracing::error!(
                session = %session.as_str(),
                tool = %call.name,
                error = %error,
                "写入工具调用失败"
            );
        }
    }
}
