//! 一次模型调用：发起、流式累积、限流退避与实时增量。定型前不落库。
//! 循环控制与定型写入在父模块。

use std::time::Duration;

use durable::{
    ContentPart, MessageId, MessageNode, RunFailure, RunFailureCode, SessionId, StreamEvent,
};
use futures_util::StreamExt;

use crate::cancel::Cancellation;
use crate::engine::{RunEngine, StepEnd, StreamOpen, ToolCallBuilder, storage_failure};
use crate::provider::{ChatChunk, ChatMessage, ProviderError};

/// 限流退避的上限。供应商不给建议时单次等待不超过它。
const MAX_RETRY_BACKOFF: Duration = Duration::from_secs(30);

/// 退避翻倍的最大次数。
const MAX_RETRY_EXPONENT: usize = 5;

impl RunEngine {
    /// 发起并读取一轮模型流：文本与推理累积在内存，工具调用片段累积到返回值。
    ///
    /// 取消或上游失败时不写入任何东西：未定型的内容只经实时增量传输。
    pub(super) async fn stream_step(
        &self,
        session: &SessionId,
        transcript: &[MessageNode],
        cancellation: &Cancellation,
    ) -> StepEnd {
        let id = match self.store.mint_message_id().await {
            Ok(id) => id,
            Err(error) => return StepEnd::Failure(storage_failure(&error)),
        };
        let messages = match self.assemble(transcript) {
            Ok(messages) => messages,
            Err(failure) => return StepEnd::Failure(failure),
        };
        let mut stream = match self.open_stream(session, &messages, cancellation).await {
            StreamOpen::Opened(stream) => stream,
            StreamOpen::Cancelled => return StepEnd::Cancelled,
            StreamOpen::Failed(error) => {
                return StepEnd::Failure(RunFailure {
                    code: failure_code(&error),
                    message: error.to_string(),
                });
            }
        };
        let mut reasoning = String::new();
        let mut text = String::new();
        let mut builders: Vec<ToolCallBuilder> = Vec::new();
        loop {
            let next = tokio::select! {
                biased;
                () = cancellation.cancelled() => return StepEnd::Cancelled,
                item = StreamExt::next(&mut stream) => item,
            };
            match next {
                None => break,
                Some(Err(error)) => {
                    return StepEnd::Failure(RunFailure {
                        code: RunFailureCode::ModelUpstream,
                        message: error.to_string(),
                    });
                }
                Some(Ok(ChatChunk::Text(delta))) => {
                    if !delta.is_empty() {
                        text.push_str(&delta);
                        self.publish_delta(session, id, DeltaKind::Text, delta);
                    }
                }
                Some(Ok(ChatChunk::Reasoning(delta))) => {
                    if !delta.is_empty() {
                        reasoning.push_str(&delta);
                        self.publish_delta(session, id, DeltaKind::Reasoning, delta);
                    }
                }
                Some(Ok(ChatChunk::ToolCall {
                    index,
                    id,
                    name,
                    arguments,
                })) => {
                    while builders.len() <= index {
                        builders.push(ToolCallBuilder::default());
                    }
                    let Some(builder) = builders.get_mut(index) else {
                        continue;
                    };
                    if let Some(id) = id {
                        builder.id = Some(id);
                    }
                    if let Some(name) = name {
                        builder.name = Some(name);
                    }
                    builder.arguments.push_str(&arguments);
                }
            }
        }
        StepEnd::Completed {
            id,
            parts: assemble_parts(reasoning, text, builders),
        }
    }

    /// 发起一次模型调用；供应商限流时按退避重试。
    ///
    /// 重试在本方法内部完成，不改变 run 的生命周期，也不写事实事件。
    pub(super) async fn open_stream(
        &self,
        session: &SessionId,
        messages: &[ChatMessage],
        cancellation: &Cancellation,
    ) -> StreamOpen {
        let mut attempt = 0usize;
        loop {
            let result = tokio::select! {
                biased;
                () = cancellation.cancelled() => return StreamOpen::Cancelled,
                result = self.provider.stream_chat(messages.to_vec(), self.tools.definitions()) => result,
            };
            let error = match result {
                Ok(stream) => return StreamOpen::Opened(stream),
                Err(error) => error,
            };
            let Some(delay) = self.retry_delay(&error, attempt) else {
                return StreamOpen::Failed(error);
            };
            attempt += 1;
            tracing::info!(
                session = %session.as_str(),
                attempt,
                limit = self.config.model_retry_limit,
                delay_ms = to_u64(delay.as_millis()),
                error = %error,
                "供应商限流，等待后重试"
            );
            self.bus.publish(StreamEvent::Retry {
                session_id: session.clone(),
                attempt: to_u32(attempt),
                max_attempts: to_u32(self.config.model_retry_limit),
                delay_ms: to_u64(delay.as_millis()),
            });
            tokio::select! {
                biased;
                () = cancellation.cancelled() => return StreamOpen::Cancelled,
                () = tokio::time::sleep(delay) => {}
            }
        }
    }

    /// 本次失败的等待时长。不重试时返回 `None`。
    fn retry_delay(&self, error: &ProviderError, attempt: usize) -> Option<Duration> {
        if !error.is_rate_limited() || attempt >= self.config.model_retry_limit {
            return None;
        }
        Some(
            error
                .retry_after()
                .unwrap_or_else(|| self.retry_backoff(attempt)),
        )
    }

    /// 指数退避：底数逐次翻倍，不超过 [`MAX_RETRY_BACKOFF`]。
    fn retry_backoff(&self, attempt: usize) -> Duration {
        let factor = 1u32 << attempt.min(MAX_RETRY_EXPONENT);
        self.config
            .model_retry_backoff
            .saturating_mul(factor)
            .min(MAX_RETRY_BACKOFF)
    }

    fn publish_delta(&self, session: &SessionId, id: MessageId, kind: DeltaKind, text: String) {
        self.bus.publish(StreamEvent::Delta {
            session_id: session.clone(),
            message_id: id,
            kind,
            text,
            timestamp: now_millis(),
        });
    }
}

/// 把本步累积的内容与工具调用变成有序块：推理、正文、调用。
fn assemble_parts(
    reasoning: String,
    text: String,
    builders: Vec<ToolCallBuilder>,
) -> Vec<ContentPart> {
    let mut parts = Vec::new();
    if !reasoning.is_empty() {
        parts.push(ContentPart::reasoning(reasoning));
    }
    if !text.is_empty() {
        parts.push(ContentPart::text(text));
    }
    for (index, builder) in builders.into_iter().enumerate() {
        let call_id = builder.id.unwrap_or_else(|| format!("call_{}", index + 1));
        parts.push(ContentPart::tool_call(
            call_id,
            builder.name.unwrap_or_default(),
            builder.arguments,
        ));
    }
    parts
}

/// 限流用稳定类别；其余上游失败仍是 `ModelUpstream`。
fn failure_code(error: &ProviderError) -> RunFailureCode {
    if error.is_rate_limited() {
        RunFailureCode::ModelRateLimited
    } else {
        RunFailureCode::ModelUpstream
    }
}

fn to_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn to_u64(value: u128) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

fn now_millis() -> i64 {
    jiff::Timestamp::now().as_millisecond()
}

use durable::DeltaKind;
