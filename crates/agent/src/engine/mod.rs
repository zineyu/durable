//! run 引擎的门面：执行循环、run 生命周期与结果归类。
//! 模型流与限流退避在 `step`，工具循环在 `tools`，请求装配在 `prompt`。

mod prompt;
mod step;
mod tools;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use durable::{
    EventBus, MessageContent, MessageNode, RunFailure, RunFailureCode, RunStart, RunStatus,
    SessionId, Store, StoreError, ToolCallRecord,
};
use tokio::sync::{Mutex, Notify};

use crate::cancel::Cancellation;
use crate::config::EngineConfig;
use crate::provider::{ChatStream, ModelProvider, ProviderError};
use crate::tools::ToolBox;

/// run 引擎：session 内串行、session 间并发、流式生成、工具循环与取消。
pub struct RunEngine {
    store: Arc<Store>,
    provider: Arc<dyn ModelProvider>,
    tools: Arc<dyn ToolBox>,
    bus: EventBus,
    config: EngineConfig,
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

/// 一次模型调用的结果：流、取消，或彻底失败。
enum StreamOpen {
    Opened(ChatStream),
    Cancelled,
    Failed(ProviderError),
}

/// 一轮流式生成的结束方式。
enum StepEnd {
    /// 流正常结束，带回累积的工具调用片段。
    Completed(Vec<ToolCallBuilder>),
    /// 被取消；已生成的部分输出已落库。
    Cancelled,
    /// 上游失败；已生成的部分输出已落库。
    Failure(RunFailure),
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
            drivers: Arc::new(Mutex::new(HashSet::new())),
            controls: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// 启动恢复：把中断的 run 标记为 failed，并为仍有队列项的 session 启动执行。
    ///
    /// # Errors
    ///
    /// - `StoreError::Database`：读取中断的 run、读取队列或提交恢复结果时数据库操作失败。
    /// - `StoreError::Serialization`：恢复过程读出或写出的记录无法序列化。
    /// - `StoreError::CorruptedData`：存储值不符合领域约束。
    /// - `StoreError::Invariant`：存储不变量被破坏。
    pub async fn recover(self: &Arc<Self>) -> Result<(), StoreError> {
        self.store.recover_running_runs().await?;
        for session in self.store.sessions_with_queue().await? {
            self.ensure_driver(session).await;
        }
        Ok(())
    }

    /// 有新入队时唤醒该 session 的执行循环。
    pub async fn notify_session(self: &Arc<Self>, session: SessionId) {
        self.ensure_driver(session).await;
    }

    /// 取消该 session 正在运行的 run，并等待其结束。
    pub async fn cancel(&self, session: &SessionId) {
        let control = self.controls.lock().await.get(session).cloned();
        if let Some(control) = control {
            control.cancellation.cancel();
            control.done.notified().await;
        }
    }

    async fn ensure_driver(self: &Arc<Self>, session: SessionId) {
        {
            let mut drivers = self.drivers.lock().await;
            if !drivers.insert(session.clone()) {
                return;
            }
        }
        let engine = Arc::clone(self);
        tokio::spawn(async move { engine.drive_loop(session).await });
    }

    async fn drive_loop(self: Arc<Self>, session: SessionId) {
        loop {
            match self.store.start_run(&session).await {
                Ok(Some(start)) => {
                    self.run_once(start).await;
                }
                Ok(None) => {
                    if self.release_driver(&session).await {
                        return;
                    }
                }
                Err(error) => {
                    tracing::error!(session = %session.as_str(), error = %error, "启动 run 失败");
                    self.drivers.lock().await.remove(&session);
                    return;
                }
            }
        }
    }

    /// 通过注销与重新登记决定是否继续执行。
    ///
    /// 返回 `true` 表示本执行循环应当结束：队列已空，或另一个执行循环已接管该 session。
    ///
    /// 注销先于复查队列，因此并发入队不会丢唤醒：入队方要么看到已注销而启动新的执行循环，
    /// 要么被本次复查重新看到。
    async fn release_driver(&self, session: &SessionId) -> bool {
        self.drivers.lock().await.remove(session);
        let queued = self
            .store
            .sessions_with_queue()
            .await
            .is_ok_and(|sessions| sessions.contains(session));
        if !queued {
            return true;
        }
        !self.drivers.lock().await.insert(session.clone())
    }

    async fn run_once(self: &Arc<Self>, start: RunStart) {
        let session = start.run.session_id.clone();
        let run = start.run.id.get();
        tracing::info!(session = %session.as_str(), run, "开始 run");
        let cancellation = Cancellation::new();
        let done = Arc::new(Notify::new());
        self.controls.lock().await.insert(
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
        if let Some(failure) = &outcome.failure {
            tracing::warn!(
                session = %session.as_str(),
                run,
                code = ?failure.code,
                message = %failure.message,
                "run 失败"
            );
        }
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
            tracing::error!(session = %session.as_str(), run, error = %error, "结束 run 失败");
        } else {
            tracing::info!(
                session = %session.as_str(),
                run,
                status = outcome.status.as_str(),
                "结束 run"
            );
        }
        self.controls.lock().await.remove(&session);
        done.notify_waiters();
    }

    async fn generate(&self, start: &RunStart, cancellation: &Cancellation) -> RunOutcome {
        let session = start.run.session_id.clone();
        let assistant_id = start.assistant.id;
        let mut text = String::new();
        let mut records: Vec<ToolCallRecord> = Vec::new();
        for step in 1..=self.config.max_steps {
            let messages = match self.step_messages(&session).await {
                Ok(messages) => messages,
                Err(failure) => return failed(failure.code, failure.message, text, records),
            };
            let builders = match self
                .stream_step(
                    &session,
                    &messages,
                    cancellation,
                    assistant_id,
                    &mut text,
                    &records,
                )
                .await
            {
                StepEnd::Completed(builders) => builders,
                StepEnd::Cancelled => return cancelled(text, records),
                StepEnd::Failure(failure) => {
                    return failed(failure.code, failure.message, text, records);
                }
            };
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
            let cancelled_run = self
                .run_tool_calls(
                    start,
                    cancellation,
                    assistant_id,
                    builders,
                    &text,
                    &mut records,
                )
                .await;
            if cancelled_run {
                return cancelled(text, records);
            }
        }
        failed(
            RunFailureCode::StepLimitExceeded,
            format!("超过 {} 步上限", self.config.max_steps),
            text,
            records,
        )
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
