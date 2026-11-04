//! run 引擎的门面：执行循环、run 生命周期与 run 动作驱动。
//! 下一步的判定在 `plan`，模型流在 `step`，工具循环在 `tools`，请求装配在 `prompt`，
//! 上下文压缩在 `compact`，进度节流在 `progress`。
//!
//! 中断现场不在启动路径单独收尾：每次开跑前的第 0 圈把它们落成事实，启动恢复与日常
//! 续跑因此走同一条路（ADR 0037）。

mod compact;
mod plan;
mod progress;
mod prompt;
mod step;
mod tools;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use durable::{
    EffectClass, EffectIntent, EventBus, MessageId, MessageRole, RunFailure, RunFailureCode, RunId,
    RunStart, RunStatus, SessionId, Store, StoreError, ToolResultState,
};
use effects::policy::{Evidence, Replay, settle};
use effects::token::{Ledger, Lost, Transition};
use tokio::sync::{Mutex, Notify};

use self::compact::{CompactionBudget, CompactionEnd};
use self::plan::NextAction;
use self::tools::ToolLoopEnd;
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

    /// 启动：为每个有活要干的 session 启动执行循环。
    ///
    /// 中断现场的处理不在这里，而在每次开跑前的第 0 圈（`drive_run` 里的
    /// `round_zero`）。启动恢复因此没有专用收尾函数：它与日常续跑走同一条路，
    /// 不可能只修好其中一边。
    ///
    /// # Errors
    ///
    /// - `StoreError::Database`：读取有活要干的 session 时数据库操作失败。
    pub async fn recover(self: &Arc<Self>) -> Result<(), StoreError> {
        for session in self.store.sessions_with_work().await? {
            self.ensure_driver(session).await;
        }
        Ok(())
    }

    /// 恢复后的不变量检查。违例只记日志：它说明状态不该出现，但进程仍应继续服务。
    ///
    /// 清单见 [`durable::InvariantKind`]，与崩溃前缀矩阵共用同一份检查。
    async fn report_invariants(&self) {
        match self.store.check_invariants().await {
            Ok(violations) => {
                for violation in violations {
                    tracing::warn!(
                        kind = violation.kind.as_str(),
                        detail = %violation.detail,
                        "恢复后的不变量违例"
                    );
                }
            }
            Err(error) => tracing::warn!(error = %error, "检查恢复后的不变量失败"),
        }
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
        let record = self.store.session_record(session).await.ok()?;
        Some(RunStart {
            run,
            host: record.host().clone(),
            workspace: record.workspace().clone(),
        })
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
        // 续跑的 run 从接管时刻开始计时：重启之前的时段没有单调读数。
        let started = Instant::now();
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
            .finish_run(
                &session,
                &start.run,
                outcome.status,
                outcome.failure,
                Some(elapsed_ms(started.elapsed())),
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
            if let Err(error) = self.store.downgrade_pending_steers(&session).await {
                tracing::error!(session = %session.as_str(), run, error = %error, "降级待插入插话失败");
            }
        }
        self.controls.lock().await.remove(&session);
        done.notify_waiters();
    }

    /// 每一步先让 [`plan`] 判定动作：接入插话、发请求、执行工具，或收尾。
    async fn drive_run(&self, start: &RunStart, cancellation: &Cancellation) -> RunOutcome {
        let session = &start.run.session_id;
        let mut compaction = CompactionBudget::new();
        // 第 0 圈：先把上次留下的半成品与未观测的结局落成事实，再让 `plan` 判定下一步。
        if let Err(error) = self.round_zero(session, start.run.id).await {
            return failed(storage_failure(&error));
        }
        loop {
            let transcript = match self.store.transcript(session).await {
                Ok(transcript) => transcript,
                Err(error) => return failed(storage_failure(&error)),
            };
            let plan = plan::next_action(&transcript, start.run.id, self.config.max_steps);
            // 步骤边界：工具执行不打断，因此只在发请求与收尾之前接入待插入插话。
            if plan.steer {
                match self.insert_pending_steers(session, start.run.id).await {
                    Ok(true) => continue,
                    Ok(false) => {}
                    Err(error) => return failed(storage_failure(&error)),
                }
            }
            match plan.action {
                NextAction::Finish => return succeeded(),
                NextAction::StepLimit => {
                    return failed(RunFailure {
                        code: RunFailureCode::StepLimitExceeded,
                        message: format!("超过 {} 步上限", self.config.max_steps),
                    });
                }
                NextAction::Request => {
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
                            if failure.code != RunFailureCode::ContextLimitExceeded {
                                return failed(failure);
                            }
                            if !compaction.take() {
                                return failed(failure);
                            }
                            tracing::info!(session = %session.as_str(), "上下文超限，压缩后重试本步");
                            match self.compact(session, &transcript, cancellation).await {
                                CompactionEnd::Compacted => {}
                                CompactionEnd::Cancelled => return cancelled(),
                                CompactionEnd::Failed(failure) => return failed(failure),
                            }
                        }
                    }
                }
                NextAction::Tools(pending) => {
                    match self.run_tool_calls(start, cancellation, pending).await {
                        Ok(ToolLoopEnd::Completed) => {}
                        Ok(ToolLoopEnd::Cancelled) => return cancelled(),
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

    /// 开跑前的第 0 圈：让流式进度入树，再按证据强度处置未观测的工具节点。
    ///
    /// 启动恢复与日常续跑都从这里进去（ADR 0037）。
    async fn round_zero(&self, session: &SessionId, run: RunId) -> Result<(), StoreError> {
        if let Some(message) = self.store.materialize_progress(session).await? {
            tracing::info!(
                session = %session.as_str(),
                message = message.id.get(),
                "流式进度入树为已中断消息"
            );
        }
        self.settle_unobserved_tools(session, run).await?;
        // 检查放在修复之后、这一步开始之前：此刻修复已完成，违例就是崩溃留下的真实形状。
        self.report_invariants().await;
        Ok(())
    }

    /// 按证据强度处置未观测的工具节点。
    ///
    /// 「已登记」一定没有执行，留给 `plan` 重跑；「已下发」按幂等分流，只有给出
    /// `GiveUp` 的才写「结果未知」。工具名找不到的节点按放弃处置：`plan` 无法重跑一条
    /// 找不到调用的节点，留着它就会永远显示「执行中」。
    async fn settle_unobserved_tools(
        &self,
        session: &SessionId,
        run: RunId,
    ) -> Result<(), StoreError> {
        let transcript = self.store.transcript(session).await?;
        for node in &transcript {
            if node.role != MessageRole::Tool {
                continue;
            }
            for (call_id, state) in node.tool_results() {
                let Some(evidence) = evidence_of(state) else {
                    continue;
                };
                let name = call_name(&transcript, call_id);
                let class = name.and_then(EffectClass::of_tool);
                let replay = match (name, class) {
                    (Some(name), Some(class)) => {
                        settle(class, evidence, idempotent(self.tools.as_ref(), name))
                    }
                    // 未登记的工具名没有副作用，重跑一定安全。
                    (Some(_), None) => Replay::Redo,
                    // 找不到调用：`plan` 无法重跑它，留着只会一直显示「执行中」。
                    (None, _) => Replay::GiveUp,
                };
                if replay == Replay::Redo {
                    tracing::info!(
                        tool = name,
                        message = node.id.get(),
                        "结局未观测的调用将重跑"
                    );
                    continue;
                }
                tracing::warn!(
                    tool = name,
                    message = node.id.get(),
                    "非幂等调用可能已经执行，结局记为未知"
                );
                let message = node.run_id.unwrap_or(run);
                let intent = match class {
                    Some(class) => EffectIntent::tool(
                        class,
                        session.clone(),
                        message,
                        node.id,
                        call_id.to_owned(),
                    ),
                    None => EffectIntent::unregistered_tool(
                        session.clone(),
                        message,
                        node.id,
                        call_id.to_owned(),
                    ),
                };
                self.store
                    .write(Transition::Lost(intent, Lost::Interrupted))
                    .await?;
            }
        }
        Ok(())
    }
}

/// 未观测态对应的证据强度。有结局的节点不参与恢复判定。
fn evidence_of(state: &ToolResultState) -> Option<Evidence> {
    match state {
        ToolResultState::Declared => Some(Evidence::Declared),
        ToolResultState::Dispatched => Some(Evidence::Dispatched),
        ToolResultState::Finished { .. } | ToolResultState::Unknown => None,
    }
}

/// 工具是否幂等。幂等是工具定义上的标记，不是执行器回答的问题。
fn idempotent(tools: &dyn ToolBox, name: &str) -> bool {
    tools
        .definitions()
        .iter()
        .any(|definition| definition.name == name && definition.idempotent)
}

/// 在 transcript 中找出某次工具调用的工具名。
fn call_name<'a>(transcript: &'a [durable::MessageNode], call_id: &str) -> Option<&'a str> {
    transcript
        .iter()
        .flat_map(durable::MessageNode::tool_calls)
        .find(|(id, ..)| *id == call_id)
        .map(|(_, name, _)| name)
}

/// 单调时钟读数转成毫秒。耗时超过 `u64` 上限时截顶，实践中不会发生。
pub(super) fn elapsed_ms(elapsed: Duration) -> u64 {
    u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
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
