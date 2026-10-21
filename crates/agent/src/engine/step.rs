//! 一次模型调用：发起、流式累积、限流退避与部分输出落库。
//! 循环控制与 run 收尾在父模块。

use std::time::Duration;

use durable::{
    MessageContent, MessageId, RunFailure, RunFailureCode, SessionId, StreamEvent, ToolCallRecord,
};

use crate::cancel::Cancellation;
use crate::engine::{RunEngine, StepEnd, StreamOpen, ToolCallBuilder};
use crate::provider::{ChatChunk, ChatMessage, ProviderError};

/// 部分输出的落库节流间隔。
const FLUSH_INTERVAL: Duration = Duration::from_millis(100);

/// 限流退避的上限。供应商不给建议时单次等待不超过它。
const MAX_RETRY_BACKOFF: Duration = Duration::from_secs(30);

/// 退避翻倍的最大次数。
const MAX_RETRY_EXPONENT: usize = 5;

impl RunEngine {
    /// 读取一轮模型流：文本累积到 `text`，工具调用片段累积到返回值。
    ///
    /// 取消或上游失败时先落库已生成的部分输出，再报告结束原因。
    pub(super) async fn stream_step(
        &self,
        session: &SessionId,
        messages: &[ChatMessage],
        cancellation: &Cancellation,
        assistant_id: MessageId,
        text: &mut String,
        records: &[ToolCallRecord],
    ) -> StepEnd {
        let mut stream = match self.open_stream(session, messages, cancellation).await {
            StreamOpen::Opened(stream) => stream,
            StreamOpen::Cancelled => return StepEnd::Cancelled,
            StreamOpen::Failed(error) => {
                return StepEnd::Failure(RunFailure {
                    code: failure_code(&error),
                    message: error.to_string(),
                });
            }
        };
        let mut builders: Vec<ToolCallBuilder> = Vec::new();
        let mut flush_tick =
            tokio::time::interval_at(tokio::time::Instant::now() + FLUSH_INTERVAL, FLUSH_INTERVAL);
        loop {
            let next = tokio::select! {
                biased;
                () = cancellation.cancelled() => {
                    self.flush(assistant_id, text, records).await;
                    return StepEnd::Cancelled;
                }
                _ = flush_tick.tick() => {
                    self.flush(assistant_id, text, records).await;
                    continue;
                }
                item = futures_util::StreamExt::next(&mut stream) => item,
            };
            match next {
                None => break,
                Some(Err(error)) => {
                    self.flush(assistant_id, text, records).await;
                    return StepEnd::Failure(RunFailure {
                        code: RunFailureCode::ModelUpstream,
                        message: error.to_string(),
                    });
                }
                Some(Ok(ChatChunk::Text(delta))) => {
                    if !delta.is_empty() {
                        text.push_str(&delta);
                        self.bus.publish(StreamEvent::Delta {
                            session_id: session.clone(),
                            message_id: assistant_id,
                            text: delta,
                            timestamp: now_millis(),
                        });
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
        self.flush(assistant_id, text, records).await;
        StepEnd::Completed(builders)
    }

    /// 发起一次模型调用；供应商限流时按退避重试。
    ///
    /// 重试在本方法内部完成，不改变 run 的生命周期，也不写事实事件。
    async fn open_stream(
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

    pub(super) async fn flush(
        &self,
        assistant_id: MessageId,
        text: &str,
        records: &[ToolCallRecord],
    ) {
        if let Err(error) = self
            .store
            .flush_message(assistant_id, MessageContent::new(text), records.to_vec())
            .await
        {
            tracing::error!(message = assistant_id.get(), error = %error, "写入部分输出失败");
        }
    }
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
