//! 上下文超限时的压缩：生成摘要并把它作为一条消息入树。
//! 触发点在父模块的执行循环，请求装配在 `prompt`，模型流在 `step`。

use durable::{ContentPart, MessageNode, RunFailure, RunFailureCode, SessionId};
use futures_util::StreamExt;

use crate::cancel::Cancellation;
use crate::engine::prompt;
use crate::engine::{RunEngine, StreamOpen};
use crate::provider::ChatChunk;

impl RunEngine {
    /// 压缩当前 transcript：用一段摘要替代历史前缀，并把摘要作为新消息入树。
    ///
    /// 失败时不写入任何东西，返回上下文超限的失败，由调用方结束 run。
    pub(super) async fn compact(
        &self,
        session: &SessionId,
        transcript: &[MessageNode],
        cancellation: &Cancellation,
    ) -> Result<(), RunFailure> {
        let messages = prompt::compaction_messages(transcript);
        let mut stream = match self.open_stream(session, &messages, cancellation).await {
            StreamOpen::Opened(stream) => stream,
            StreamOpen::Cancelled => return Err(context_limit("压缩被取消，摘要没有生成")),
            StreamOpen::Failed(error) => {
                return Err(context_limit(&format!("压缩请求失败: {error}")));
            }
        };
        let mut summary = String::new();
        loop {
            let next = tokio::select! {
                biased;
                () = cancellation.cancelled() => return Err(context_limit("压缩被取消，摘要没有生成")),
                item = StreamExt::next(&mut stream) => item,
            };
            match next {
                None => break,
                Some(Err(error)) => {
                    return Err(context_limit(&format!("压缩流失败: {error}")));
                }
                Some(Ok(ChatChunk::Text(delta))) => summary.push_str(&delta),
                Some(Ok(ChatChunk::Reasoning(_) | ChatChunk::ToolCall { .. })) => {}
            }
        }
        let summary = summary.trim();
        if summary.is_empty() {
            return Err(context_limit("压缩没有产生摘要"));
        }
        let id = self
            .store
            .mint_message_id()
            .await
            .map_err(|error| context_limit(&error.to_string()))?;
        self.store
            .append_compaction(session, id, vec![ContentPart::text(summary)])
            .await
            .map_err(|error| context_limit(&error.to_string()))?;
        Ok(())
    }
}

fn context_limit(message: &str) -> RunFailure {
    RunFailure {
        code: RunFailureCode::ContextLimitExceeded,
        message: message.to_owned(),
    }
}
