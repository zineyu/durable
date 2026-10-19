use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use durable::{
    EventBus, MessageContent, MessageNode, MessageRole, RunFailure, RunFailureCode, RunStart,
    RunStatus, SessionId, Store, StoreError, StreamEvent, ToolCallRecord, ToolOutcome,
};
use tokio::sync::{Notify, Semaphore};

use crate::cancel::Cancellation;
use crate::config::EngineConfig;
use crate::provider::{ChatChunk, ChatMessage, ChatRole, ChatToolCall, ModelProvider};
use crate::tools::{ToolBox, ToolCall, ToolContext};

/// 部分输出的落库节流间隔。
const FLUSH_INTERVAL: Duration = Duration::from_millis(100);

const SYSTEM_PROMPT: &str = "你是运行在本机 workspace 根目录中的 coding agent。\
使用提供的工具读取文件、写入文件、精确编辑文件与执行命令；所有路径都必须位于 workspace 根内。\
禁止猜测文件内容，需要时先读取文件。回答保持简洁。";

/// run 引擎：全局并发槽位、session 间公平轮转、流式生成、工具循环与取消。
pub struct RunEngine {
    store: Arc<Store>,
    provider: Arc<dyn ModelProvider>,
    tools: Arc<dyn ToolBox>,
    bus: EventBus,
    config: EngineConfig,
    permits: Arc<Semaphore>,
    drivers: Arc<Mutex<HashSet<SessionId>>>,
    controls: Arc<Mutex<HashMap<SessionId, RunControl>>>,
}

#[derive(Clone)]
struct RunControl {
    cancellation: Cancellation,
    done: Arc<Notify>,
}

struct RunOutcome {
    status: RunStatus,
    failure: Option<RunFailure>,
    text: MessageContent,
    tool_calls: Vec<ToolCallRecord>,
}

#[derive(Default)]
struct ToolCallBuilder {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

impl RunEngine {
    pub fn new(
        store: Arc<Store>,
        provider: Arc<dyn ModelProvider>,
        tools: Arc<dyn ToolBox>,
        config: EngineConfig,
    ) -> Arc<Self> {
        let bus = store.events().clone();
        Arc::new(Self {
            store,
            provider,
            tools,
            bus,
            config,
            permits: Arc::new(Semaphore::new(config.max_concurrency)),
            drivers: Arc::new(Mutex::new(HashSet::new())),
            controls: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// 启动恢复：把中断的 run 标记为 failed，并为仍有队列项的 session 启动执行。
    ///
    /// 失败：存储读取或写入失败。
    pub async fn recover(self: &Arc<Self>) -> Result<(), StoreError> {
        self.store.recover_running_runs().await?;
        for session in self.store.sessions_with_queue().await? {
            self.ensure_driver(session);
        }
        Ok(())
    }

    /// 有新入队时唤醒该 session 的执行循环。
    pub fn notify_session(self: &Arc<Self>, session: SessionId) {
        self.ensure_driver(session);
    }

    /// 取消该 session 正在运行的 run，并等待其结束。
    pub async fn cancel(&self, session: &SessionId) {
        let control = self
            .controls
            .lock()
            .expect("controls 锁")
            .get(session)
            .cloned();
        if let Some(control) = control {
            control.cancellation.cancel();
            control.done.notified().await;
        }
    }

    fn ensure_driver(self: &Arc<Self>, session: SessionId) {
        {
            let mut drivers = self.drivers.lock().expect("drivers 锁");
            if !drivers.insert(session.clone()) {
                return;
            }
        }
        let engine = Arc::clone(self);
        tokio::spawn(async move { engine.drive_loop(session).await });
    }

    async fn drive_loop(self: Arc<Self>, session: SessionId) {
        loop {
            let permit = match self.permits.clone().acquire_owned().await {
                Ok(permit) => permit,
                Err(_) => break,
            };
            match self.store.start_run(&session).await {
                Ok(Some(start)) => {
                    self.run_once(start).await;
                    drop(permit);
                }
                Ok(None) => {
                    drop(permit);
                    break;
                }
                Err(error) => {
                    eprintln!("启动 run 失败: {error}");
                    drop(permit);
                    break;
                }
            }
        }
        self.drivers.lock().expect("drivers 锁").remove(&session);
        let queued = self
            .store
            .sessions_with_queue()
            .await
            .map(|sessions| sessions.contains(&session))
            .unwrap_or(false);
        if queued {
            self.ensure_driver(session);
        }
    }

    async fn run_once(self: &Arc<Self>, start: RunStart) {
        let session = start.run.session_id.clone();
        let cancellation = Cancellation::new();
        let done = Arc::new(Notify::new());
        self.controls.lock().expect("controls 锁").insert(
            session.clone(),
            RunControl {
                cancellation: cancellation.clone(),
                done: Arc::clone(&done),
            },
        );
        let outcome = self.generate(&start, &cancellation).await;
        let assistant = MessageNode {
            text: outcome.text,
            tool_calls: outcome.tool_calls,
            ..start.assistant.clone()
        };
        if let Err(error) = self
            .store
            .finish_run(
                &session,
                &start.run,
                outcome.status,
                outcome.failure,
                assistant,
            )
            .await
        {
            eprintln!("结束 run 失败: {error}");
        }
        self.controls.lock().expect("controls 锁").remove(&session);
        done.notify_waiters();
    }

    async fn generate(&self, start: &RunStart, cancellation: &Cancellation) -> RunOutcome {
        let session = start.run.session_id.clone();
        let assistant_id = start.assistant.id;
        let mut text = String::new();
        let mut records: Vec<ToolCallRecord> = Vec::new();
        for step in 1..=self.config.max_steps {
            let transcript = match self.store.transcript(&session).await {
                Ok(transcript) => transcript,
                Err(error) => {
                    return failed(
                        RunFailureCode::ModelUpstream,
                        error.to_string(),
                        text,
                        records,
                    );
                }
            };
            let messages = build_messages(&transcript);
            if char_count(&messages) > self.config.context_char_limit {
                return failed(
                    RunFailureCode::ContextLimitExceeded,
                    format!("上下文超过 {} 字符上限", self.config.context_char_limit),
                    text,
                    records,
                );
            }
            let stream = tokio::select! {
                biased;
                _ = cancellation.cancelled() => return cancelled(text, records),
                result = self.provider.stream_chat(messages, self.tools.definitions()) => result,
            };
            let mut stream = match stream {
                Ok(stream) => stream,
                Err(error) => {
                    return failed(
                        RunFailureCode::ModelUpstream,
                        error.to_string(),
                        text,
                        records,
                    );
                }
            };
            let mut builders: Vec<ToolCallBuilder> = Vec::new();
            let mut flush_tick = tokio::time::interval_at(
                tokio::time::Instant::now() + FLUSH_INTERVAL,
                FLUSH_INTERVAL,
            );
            loop {
                let next = tokio::select! {
                    biased;
                    _ = cancellation.cancelled() => {
                        self.flush(assistant_id, &text, &records).await;
                        return cancelled(text, records);
                    }
                    _ = flush_tick.tick() => {
                        self.flush(assistant_id, &text, &records).await;
                        continue;
                    }
                    item = futures_util::StreamExt::next(&mut stream) => item,
                };
                match next {
                    None => break,
                    Some(Err(error)) => {
                        self.flush(assistant_id, &text, &records).await;
                        return failed(
                            RunFailureCode::ModelUpstream,
                            error.to_string(),
                            text,
                            records,
                        );
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
                        let builder = &mut builders[index];
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
            self.flush(assistant_id, &text, &records).await;
            if builders.is_empty() {
                return RunOutcome {
                    status: RunStatus::Succeeded,
                    failure: None,
                    text: MessageContent::new(text),
                    tool_calls: records,
                };
            }
            if step == self.config.max_steps {
                return failed(
                    RunFailureCode::StepLimitExceeded,
                    format!("超过 {} 步上限", self.config.max_steps),
                    text,
                    records,
                );
            }
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
                let outcome = tokio::select! {
                    biased;
                    _ = cancellation.cancelled() => {
                        self.flush(assistant_id, &text, &records).await;
                        return cancelled(text, records);
                    }
                    outcome = self.tools.execute(&call, &context) => outcome,
                };
                records.push(ToolCallRecord {
                    id: call.id,
                    name: call.name,
                    arguments: call.arguments,
                    outcome,
                });
            }
            self.flush(assistant_id, &text, &records).await;
        }
        failed(
            RunFailureCode::StepLimitExceeded,
            format!("超过 {} 步上限", self.config.max_steps),
            text,
            records,
        )
    }

    async fn flush(
        &self,
        assistant_id: durable::MessageId,
        text: &str,
        records: &[ToolCallRecord],
    ) {
        if let Err(error) = self
            .store
            .flush_message(assistant_id, MessageContent::new(text), records.to_vec())
            .await
        {
            eprintln!("写入部分输出失败: {error}");
        }
    }
}

fn cancelled(text: String, tool_calls: Vec<ToolCallRecord>) -> RunOutcome {
    RunOutcome {
        status: RunStatus::Cancelled,
        failure: None,
        text: MessageContent::new(text),
        tool_calls,
    }
}

fn failed(
    code: RunFailureCode,
    message: String,
    text: String,
    tool_calls: Vec<ToolCallRecord>,
) -> RunOutcome {
    RunOutcome {
        status: RunStatus::Failed,
        failure: Some(RunFailure { code, message }),
        text: MessageContent::new(text),
        tool_calls,
    }
}

fn build_messages(transcript: &[MessageNode]) -> Vec<ChatMessage> {
    let mut messages = vec![ChatMessage::text(ChatRole::System, SYSTEM_PROMPT)];
    for node in transcript {
        match node.role {
            MessageRole::User => {
                messages.push(ChatMessage::text(ChatRole::User, node.text.as_str()))
            }
            MessageRole::Assistant => {
                messages.push(ChatMessage {
                    role: ChatRole::Assistant,
                    content: node.text.as_str().to_owned(),
                    tool_calls: node
                        .tool_calls
                        .iter()
                        .map(|record| ChatToolCall {
                            id: record.id.clone(),
                            name: record.name.clone(),
                            arguments: record.arguments.clone(),
                        })
                        .collect(),
                    tool_call_id: None,
                });
                for record in &node.tool_calls {
                    messages.push(ChatMessage {
                        role: ChatRole::Tool,
                        content: tool_result_content(&record.outcome),
                        tool_calls: Vec::new(),
                        tool_call_id: Some(record.id.clone()),
                    });
                }
            }
        }
    }
    messages
}

fn tool_result_content(outcome: &ToolOutcome) -> String {
    match outcome {
        ToolOutcome::Succeeded { content } => content.clone(),
        ToolOutcome::Failed { code, message } => {
            serde_json::json!({ "error": code, "message": message }).to_string()
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

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("系统时间在 UNIX_EPOCH 之后")
        .as_millis() as i64
}
