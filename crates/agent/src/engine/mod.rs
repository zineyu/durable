//! run 引擎的门面：执行循环、run 生命周期与续跑点驱动。
//! 模型流在 `step`，工具循环在 `tools`，请求装配在 `prompt`，续跑推导在 `recover`，
//! 上下文压缩在 `compact`，进度节流在 `progress`。

mod compact;
mod progress;
mod prompt;
mod recover;
mod step;
mod tools;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use durable::{
    EventBus, MessageId, MessageRole, RunFailure, RunFailureCode, RunId, RunStart, RunStatus,
    SessionId, Store, StoreError, ToolResultState,
};
use tokio::sync::{Mutex, Notify};

use self::recover::ResumePoint;
use crate::config::EngineConfig;
use crate::link::ToolBox;
use crate::provider::ModelProvider;
use durable::Cancellation;

/// run 引擎：session 内串行、session 间并发、流式生成、工具循环与续跑。
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

/// run 的结束方式。
struct RunOutcome {
    status: RunStatus,
    failure: Option<RunFailure>,
}

/// 一次模型调用的流：打开、取消，或彻底失败。
enum StreamOpen {
    Opened(crate::provider::ChatStream),
    Cancelled,
    Failed(crate::provider::ProviderError),
}

/// 一次模型调用的结束方式。
enum StepEnd {
    /// 流正常结束，带回本步定型的消息 id 与内容块。
    Completed {
        id: MessageId,
        parts: Vec<durable::ContentPart>,
    },
    /// 被取消；已写入进度的部分输出保留，恢复时入树为已中断消息。
    Cancelled,
    /// 上游失败。
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

    /// 启动恢复：把中断的工具节点标成结局，把流式进度入树为已中断消息，
    /// 并为需要执行的 session 启动驱动。
    ///
    /// # Errors
    ///
    /// - `StoreError::Database`：读取中断的 run、消息或队列时数据库操作失败。
    /// - `StoreError::Serialization`：恢复过程写出的记录无法序列化。
    /// - `StoreError::CorruptedData`：存储值不符合领域约束。
    /// - `StoreError::Invariant`：存储不变量被破坏。
    pub async fn recover(self: &Arc<Self>) -> Result<(), StoreError> {
        for session in self.store.sessions_with_work().await? {
            self.resolve_interrupted_tools(&session).await?;
            self.resolve_interrupted_progress(&session).await?;
            self.ensure_driver(session).await;
        }
        Ok(())
    }

    /// 启动恢复时把流式进度入树为已中断消息。续跑推导随后从这条消息之后继续。
    async fn resolve_interrupted_progress(&self, session: &SessionId) -> Result<(), StoreError> {
        let Some(message) = self.store.materialize_progress(session).await? else {
            return Ok(());
        };
        tracing::info!(
            session = %session.as_str(),
            message = message.id.get(),
            "流式进度入树为已中断消息"
        );
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
            if let Some(start) = self.resume_existing(&session).await {
                self.run_once(start).await;
                continue;
            }
            match self.store.start_run(&session).await {
                Ok(Some(start)) => self.run_once(start).await,
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

    /// 进程重启后接管仍是 running 的 run。
    async fn resume_existing(&self, session: &SessionId) -> Option<RunStart> {
        let run = self.store.running_run(session).await.ok().flatten()?;
        let workspace = self
            .store
            .session_record(session)
            .await
            .ok()?
            .workspace()
            .clone();
        Some(RunStart { run, workspace })
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
        let outcome = self.drive_run(&start, &cancellation).await;
        if matches!(outcome.status, RunStatus::Cancelled)
            && let Err(error) = self.store.materialize_progress(&session).await
        {
            tracing::error!(session = %session.as_str(), run, error = %error, "取消时入树部分输出失败");
        }
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
            .finish_run(&session, &start.run, outcome.status, outcome.failure)
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
            if let Err(error) = self.store.downgrade_pending_steers(&session).await {
                tracing::error!(session = %session.as_str(), run, error = %error, "降级待插入插话失败");
            }
        }
        self.controls.lock().await.remove(&session);
        done.notify_waiters();
    }

    /// 每一步先看树尾决定动作：发请求、执行工具，或收尾。
    async fn drive_run(&self, start: &RunStart, cancellation: &Cancellation) -> RunOutcome {
        let session = &start.run.session_id;
        let mut steps: Option<usize> = None;
        let mut compacted = false;
        loop {
            let transcript = match self.store.transcript(session).await {
                Ok(transcript) => transcript,
                Err(error) => return failed(storage_failure(&error)),
            };
            let executed = *steps.get_or_insert_with(|| {
                transcript
                    .iter()
                    .filter(|node| {
                        node.role == MessageRole::Assistant && node.run_id == Some(start.run.id)
                    })
                    .count()
            });
            let resume = recover::resume_point(&transcript);
            // 步数上限先于接入：插话留给下一次 run，不白白消耗在一次必定失败的请求上。
            if matches!(resume, ResumePoint::Request) && executed >= self.config.max_steps {
                return failed(RunFailure {
                    code: RunFailureCode::StepLimitExceeded,
                    message: format!("超过 {} 步上限", self.config.max_steps),
                });
            }
            // 步骤边界：工具执行不打断，因此只在发请求与收尾之前接入待插入插话。
            if !matches!(resume, ResumePoint::Tools(_)) {
                match self.insert_pending_steers(session, start.run.id).await {
                    Ok(true) => continue,
                    Ok(false) => {}
                    Err(error) => return failed(storage_failure(&error)),
                }
            }
            match resume {
                ResumePoint::Finish => return succeeded(),
                ResumePoint::Request => {
                    steps = Some(executed + 1);
                    match self
                        .stream_step(session, start.run.id, &transcript, cancellation)
                        .await
                    {
                        StepEnd::Completed { id, parts } => {
                            if let Err(error) = self
                                .store
                                .append_assistant(session, start.run.id, id, parts)
                                .await
                            {
                                return failed(storage_failure(&error));
                            }
                        }
                        StepEnd::Cancelled => return cancelled(),
                        StepEnd::Failure(failure) => {
                            if failure.code == RunFailureCode::ContextLimitExceeded && !compacted {
                                compacted = true;
                                tracing::info!(session = %session.as_str(), "上下文超限，压缩后重试本步");
                                if let Err(failure) =
                                    self.compact(session, &transcript, cancellation).await
                                {
                                    return failed(failure);
                                }
                                continue;
                            }
                            return failed(failure);
                        }
                    }
                }
                ResumePoint::Tools(pending) => {
                    match self.run_tool_calls(start, cancellation, pending).await {
                        Ok(true) => {}
                        Ok(false) => return cancelled(),
                        Err(failure) => return failed(failure),
                    }
                }
            }
        }
    }

    /// 步骤边界：把待插入插话接入链尾并绑定当前 run。返回是否插入了消息。
    ///
    /// 插入会改写 active leaf，因此调用方必须重新读 transcript 再决定下一步。
    async fn insert_pending_steers(
        &self,
        session: &SessionId,
        run: RunId,
    ) -> Result<bool, StoreError> {
        Ok(!self
            .store
            .apply_pending_steers(session, run)
            .await?
            .is_empty())
    }

    /// 启动恢复时处理执行中的工具节点：非幂等工具的结局写为未知，等待续跑时重跑幂等工具。
    async fn resolve_interrupted_tools(&self, session: &SessionId) -> Result<(), StoreError> {
        let transcript = self.store.transcript(session).await?;
        for node in &transcript {
            if node.role != MessageRole::Tool {
                continue;
            }
            for (call_id, state) in node.tool_results() {
                if !matches!(state, ToolResultState::Running) {
                    continue;
                }
                let Some(name) = call_name(&transcript, call_id) else {
                    tracing::warn!(message = node.id.get(), "执行中的工具节点找不到调用记录");
                    continue;
                };
                if self.tools.idempotent(name) {
                    tracing::info!(tool = name, message = node.id.get(), "重跑幂等工具");
                    continue;
                }
                tracing::warn!(
                    tool = name,
                    message = node.id.get(),
                    "非幂等工具结局记为未知"
                );
                self.store
                    .finish_tool(session, node.id, call_id, ToolResultState::Unknown)
                    .await?;
            }
        }
        Ok(())
    }
}

/// 在 transcript 中找出某次工具调用的工具名。
fn call_name<'a>(transcript: &'a [durable::MessageNode], call_id: &str) -> Option<&'a str> {
    transcript
        .iter()
        .flat_map(durable::MessageNode::tool_calls)
        .find(|(id, ..)| *id == call_id)
        .map(|(_, name, _)| name)
}

fn succeeded() -> RunOutcome {
    RunOutcome {
        status: RunStatus::Succeeded,
        failure: None,
    }
}

fn cancelled() -> RunOutcome {
    RunOutcome {
        status: RunStatus::Cancelled,
        failure: None,
    }
}

fn failed(failure: RunFailure) -> RunOutcome {
    RunOutcome {
        status: RunStatus::Failed,
        failure: Some(failure),
    }
}

/// 把内核的错误转成 run 失败。引擎内任何 [`StoreError`] 都是本地存储失败。
pub(super) fn storage_failure(error: &StoreError) -> RunFailure {
    RunFailure {
        code: RunFailureCode::StorageFailed,
        message: error.to_string(),
    }
}
