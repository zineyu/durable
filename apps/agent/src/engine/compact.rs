//! 上下文超限时的压缩：生成摘要并把它作为一条消息入树。
//! 触发点在父模块的执行循环，请求装配在 `prompt`，模型流在 `step`。

use std::time::Duration;

use durable::{
    Cancellation, ContentPart, EffectIntent, EffectObservation, MessageNode, RunFailure,
    RunFailureCode, SessionId,
};
use effects::token::{Dispatched, Ledger, Lost, Transition, declare, dispatch};
use futures_util::StreamExt;

use crate::engine::prompt;
use crate::engine::{RunEngine, StreamOpen, storage_failure, timeout_failure};
use crate::provider::{ChatChunk, ChatMessage, ProviderError};

/// 一次 run 的压缩机会：只压缩一次，防止「压缩后仍超限」反复重试。
///
/// 机会在尝试之前取用，因此压缩请求本身失败也不会重试。
pub(super) struct CompactionBudget {
    used: bool,
}

impl CompactionBudget {
    pub(super) fn new() -> Self {
        Self { used: false }
    }

    /// 取用一次压缩机会。已经用过时返回 `false`。
    pub(super) fn take(&mut self) -> bool {
        if self.used {
            return false;
        }
        self.used = true;
        true
    }
}

/// 压缩的结束方式。
///
/// 取消单独成一支：压缩期间取消不是上下文超限，run 应当以 `cancelled` 结束。
pub(super) enum CompactionEnd {
    /// 摘要已入树，可以重试本步。
    Compacted,
    /// 压缩途中被取消。
    Cancelled,
    /// 压缩失败，run 按这个失败结束。
    Failed(RunFailure),
}

/// 摘要请求的结束方式。
enum Summary {
    /// 得到了摘要。
    Ready(Vec<ContentPart>),
    /// 请求成功但摘要为空。
    Empty,
    /// 途中被取消。
    Cancelled,
    /// 客户端时限到点。
    Timeout(Duration),
    /// 请求失败，附带已经拼好的说明。
    Failed(String),
}

impl RunEngine {
    /// 压缩当前 transcript：用一段摘要替代历史前缀，并把摘要作为新消息入树。
    ///
    /// 失败时不写入任何东西。取消时同样不写入，但与失败分开报告。
    ///
    /// 证据与模型步骤同一套：登记与发货不写行，收货把摘要入树，超时与取消写成丢失观测。
    pub(super) async fn compact(
        &self,
        session: &SessionId,
        transcript: &[MessageNode],
        cancellation: &Cancellation,
    ) -> CompactionEnd {
        let messages = prompt::compaction_messages(transcript);
        let id = match self.store.mint_message_id().await {
            Ok(id) => id,
            Err(error) => return CompactionEnd::Failed(storage_failure(&error)),
        };
        let intent = EffectIntent::compaction(session.clone(), id);
        let declared = match declare(self.store.as_ref(), intent).await {
            Ok(declared) => declared,
            Err(error) => return CompactionEnd::Failed(storage_failure(&error)),
        };
        let dispatched = match dispatch(self.store.as_ref(), declared).await {
            Ok(dispatched) => dispatched,
            Err(error) => return CompactionEnd::Failed(storage_failure(&error)),
        };
        let intent = dispatched.intent().clone();
        match self
            .read_summary(dispatched, session, &messages, cancellation)
            .await
        {
            Summary::Ready(parts) => {
                let observation = EffectObservation::Compaction { parts };
                match self
                    .store
                    .write(Transition::Observed(intent, observation))
                    .await
                {
                    Ok(()) => CompactionEnd::Compacted,
                    Err(error) => CompactionEnd::Failed(storage_failure(&error)),
                }
            }
            Summary::Empty => CompactionEnd::Failed(context_limit("压缩没有产生摘要")),
            Summary::Cancelled => {
                self.lose_observation(session, intent, Lost::Cancelled)
                    .await;
                CompactionEnd::Cancelled
            }
            Summary::Timeout(limit) => {
                self.lose_observation(session, intent, Lost::Timeout).await;
                CompactionEnd::Failed(timeout_failure(limit))
            }
            // 上游失败没有观测可写：没有摘要就入不了树。
            Summary::Failed(message) => CompactionEnd::Failed(context_limit(&message)),
        }
    }

    /// 读一次摘要流。许可证按值传入：只有已下发的请求才执行到这里（ADR 0037）。
    async fn read_summary(
        &self,
        _witness: Dispatched<EffectIntent>,
        session: &SessionId,
        messages: &[ChatMessage],
        cancellation: &Cancellation,
    ) -> Summary {
        let mut stream = match self.open_stream(session, messages, cancellation).await {
            StreamOpen::Opened(stream) => stream,
            StreamOpen::Cancelled => return Summary::Cancelled,
            StreamOpen::Failed(ProviderError::Timeout(limit)) => return Summary::Timeout(limit),
            StreamOpen::Failed(error) => {
                return Summary::Failed(format!("压缩请求失败: {error}"));
            }
        };
        let mut summary = String::new();
        loop {
            let next = tokio::select! {
                biased;
                () = cancellation.cancelled() => return Summary::Cancelled,
                item = StreamExt::next(&mut stream) => item,
            };
            match next {
                None => break,
                Some(Err(ProviderError::Timeout(limit))) => return Summary::Timeout(limit),
                Some(Err(error)) => {
                    return Summary::Failed(format!("压缩流失败: {error}"));
                }
                Some(Ok(ChatChunk::Text(delta))) => summary.push_str(&delta),
                Some(Ok(ChatChunk::Reasoning(_) | ChatChunk::ToolCall { .. })) => {}
            }
        }
        let summary = summary.trim();
        if summary.is_empty() {
            return Summary::Empty;
        }
        Summary::Ready(vec![ContentPart::text(summary)])
    }
}

fn context_limit(message: &str) -> RunFailure {
    RunFailure {
        code: RunFailureCode::ContextLimitExceeded,
        message: message.to_owned(),
    }
}
