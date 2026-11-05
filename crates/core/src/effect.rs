//! 副作用类别表与证据落点：领域内核的六类对外动作，以及把一次证据写进现有领域行的
//! 唯一入口。未登记的工具名不属于六类，它没有副作用，落点仍是 tool 节点。
//! 策略合成、许可证与覆盖率在 `effects`；写入线与写命令在 `store`；run 的失败码在
//! `run`；工具定义与工具结局在 `tool`。超时不在这张表里，它归各介质层（ADR 0059）。

use std::future::Future;

use effects::policy::{Effect, EffectScope, Evidence, Replay};
use effects::token::{Ledger, Lost, Transition};

use crate::error::StoreError;
use crate::id::{MessageId, RunId, SessionId};
use crate::message::ContentPart;
use crate::store::Store;
use crate::tool::ToolResultState;

effects::effect_table! {
    /// 领域内核的一类对外动作。清单的唯一定义就是这张表：枚举、全量清单与稳定标签
    /// 都由宏生成，因此不存在「清单落后于变体」这个形状（ADR 0062）。
    pub enum EffectClass {
        /// 一次模型步骤请求。
        ModelStep => "model_step",
        /// 一次压缩请求。
        Compaction => "compaction",
        /// 一次 `read_file` 调用。
        ReadFile => "read_file",
        /// 一次 `write_file` 调用。
        WriteFile => "write_file",
        /// 一次 `edit_file` 调用。
        EditFile => "edit_file",
        /// 一次 `bash` 调用。
        Bash => "bash",
    }
}

impl Effect for EffectClass {
    const ALL: &'static [Self] = Self::ALL_CLASSES;

    fn name(self) -> &'static str {
        self.label()
    }

    /// 模型步骤与压缩重发没有副作用；四个工具按幂等标记分流，因为幂等是工具定义上的
    /// 性质，不是类别的性质（ADR 0052、ADR 0060）。
    fn replay(self, _evidence: Evidence) -> Replay {
        match self {
            Self::ModelStep | Self::Compaction => Replay::Redo,
            Self::ReadFile | Self::WriteFile | Self::EditFile | Self::Bash => Replay::IfIdempotent,
        }
    }

    fn matrix_scope(self) -> EffectScope {
        EffectScope::Covered
    }
}

impl EffectClass {
    /// 四个工具类。它们的证据落在 tool 节点上，模型与压缩落在消息入树上。
    #[must_use]
    pub fn is_tool(self) -> bool {
        match self {
            Self::ReadFile | Self::WriteFile | Self::EditFile | Self::Bash => true,
            Self::ModelStep | Self::Compaction => false,
        }
    }

    /// 按工具名取类别。未登记的工具返回 `None`。
    #[must_use]
    pub fn of_tool(name: &str) -> Option<Self> {
        [Self::ReadFile, Self::WriteFile, Self::EditFile, Self::Bash]
            .into_iter()
            .find(|class| class.name() == name)
    }
}

/// 一次动作的意图：它是哪一类，以及它落在哪条领域行上。
///
/// 类别与落点的搭配由构造入口固定，因此不合法的搭配造不出来；意图与观测的搭配
/// 由 [`Store`] 的落点表判定。
///
/// 类别可以缺席：未登记的工具名不属于六类，它一定没有副作用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectIntent {
    class: Option<EffectClass>,
    subject: Subject,
}

/// 意图指向的领域行。
#[derive(Debug, Clone, PartialEq, Eq)]
enum Subject {
    /// 一次模型步骤：所属 run 与将要定型的消息。
    ModelStep {
        session: SessionId,
        run: RunId,
        message: MessageId,
    },
    /// 一次压缩：将要入树的压缩消息。
    Compaction {
        session: SessionId,
        message: MessageId,
    },
    /// 一次工具调用：tool 节点与调用 id。
    Tool {
        session: SessionId,
        run: RunId,
        message: MessageId,
        call_id: String,
    },
}

impl Subject {
    fn name(&self) -> &'static str {
        match self {
            Self::ModelStep { .. } => "模型步骤",
            Self::Compaction { .. } => "压缩",
            Self::Tool { .. } => "工具调用",
        }
    }
}

impl EffectIntent {
    /// 一次模型步骤的意图。
    #[must_use]
    pub fn model_step(session: SessionId, run: RunId, message: MessageId) -> Self {
        Self {
            class: Some(EffectClass::ModelStep),
            subject: Subject::ModelStep {
                session,
                run,
                message,
            },
        }
    }

    /// 一次压缩的意图。
    #[must_use]
    pub fn compaction(session: SessionId, message: MessageId) -> Self {
        Self {
            class: Some(EffectClass::Compaction),
            subject: Subject::Compaction { session, message },
        }
    }

    /// 一次工具调用的意图。类别必须是四个工具之一，判定在落点表里。
    #[must_use]
    pub fn tool(
        class: EffectClass,
        session: SessionId,
        run: RunId,
        message: MessageId,
        call_id: impl Into<String>,
    ) -> Self {
        Self {
            class: Some(class),
            subject: Subject::Tool {
                session,
                run,
                message,
                call_id: call_id.into(),
            },
        }
    }

    /// 未登记工具名的意图：它不属于六类，因此不计入覆盖率。
    ///
    /// 执行端对未登记的名字只答失败，这条调用一定没有副作用，重放因此按 `Redo` 处置。
    /// 证据仍落在 tool 节点上，否则 assistant 的调用与它的结果不再配对。
    #[must_use]
    pub fn unregistered_tool(
        session: SessionId,
        run: RunId,
        message: MessageId,
        call_id: impl Into<String>,
    ) -> Self {
        Self {
            class: None,
            subject: Subject::Tool {
                session,
                run,
                message,
                call_id: call_id.into(),
            },
        }
    }

    /// 本次动作的类别。未登记的工具名没有类别。
    #[must_use]
    pub fn class(&self) -> Option<EffectClass> {
        self.class
    }
}

/// 一次动作的观测：动作做完后要落到领域行上的东西。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectObservation {
    /// 模型步骤定型：完整内容块一次入树。
    Assistant { parts: Vec<ContentPart> },
    /// 压缩产出：摘要块入树。
    Compaction { parts: Vec<ContentPart> },
    /// 工具结局：写回同一个 tool 节点。
    Tool(ToolResultState),
}

impl EffectObservation {
    fn name(&self) -> &'static str {
        match self {
            Self::Assistant { .. } => "assistant 消息",
            Self::Compaction { .. } => "压缩消息",
            Self::Tool(_) => "工具结局",
        }
    }
}

impl Ledger for Store {
    type Intent = EffectIntent;
    type Observation = EffectObservation;
    type Error = StoreError;

    fn write(
        &self,
        transition: Transition<Self::Intent, Self::Observation>,
    ) -> impl Future<Output = Result<(), Self::Error>> {
        self.land(transition)
    }
}

impl Store {
    /// 本进程里已登记过的副作用类别，每个一次，按首次出现的顺序。
    #[must_use]
    pub fn effects_declared(&self) -> Vec<EffectClass> {
        self.coverage.declared()
    }

    /// 声明覆盖却没出现过的类别名。为空才算矩阵有效。
    #[must_use]
    pub fn effect_gaps(&self) -> Vec<&'static str> {
        self.coverage.gaps()
    }

    /// 写入一次证据。六类与未登记工具名的落点判定只在这里。
    ///
    /// # Errors
    ///
    /// - `StoreError::Invariant`：工具意图带了非工具的类别，或观测不属于该类别。
    /// - 其余错误来自各落点的写入方法。
    async fn land(
        &self,
        transition: Transition<EffectIntent, EffectObservation>,
    ) -> Result<(), StoreError> {
        match transition {
            Transition::Declared(intent) => {
                let class = intent.class();
                self.land_declared(intent).await?;
                if let Some(class) = class {
                    self.coverage.record(class);
                }
                Ok(())
            }
            Transition::Dispatched(intent) => self.land_dispatched(intent).await,
            Transition::Observed(intent, observation) => {
                self.land_observed(intent, observation).await
            }
            Transition::Lost(intent, lost) => self.land_lost(intent, lost).await,
        }
    }

    /// 已登记：工具类让 tool 节点入树；模型步骤与压缩不写行，意图由消息 id 与进度表达。
    async fn land_declared(&self, intent: EffectIntent) -> Result<(), StoreError> {
        let EffectIntent { class, subject } = intent;
        match subject {
            Subject::Tool {
                session,
                run,
                message,
                call_id,
            } => {
                expect_tool(class)?;
                self.append_tool(&session, run, message, &call_id)
                    .await
                    .map(|_| ())
            }
            Subject::ModelStep { .. } | Subject::Compaction { .. } => Ok(()),
        }
    }

    /// 已下发：工具类把块改成已下发；模型步骤与压缩不写行。
    async fn land_dispatched(&self, intent: EffectIntent) -> Result<(), StoreError> {
        let EffectIntent { class, subject } = intent;
        match subject {
            Subject::Tool {
                session,
                message,
                call_id,
                ..
            } => {
                expect_tool(class)?;
                self.set_tool_state(&session, message, &call_id, ToolResultState::Dispatched)
                    .await
                    .map(|_| ())
            }
            Subject::ModelStep { .. } | Subject::Compaction { .. } => Ok(()),
        }
    }

    /// 有观测：工具类写结局；模型步骤与压缩把消息入树。
    async fn land_observed(
        &self,
        intent: EffectIntent,
        observation: EffectObservation,
    ) -> Result<(), StoreError> {
        let EffectIntent { class, subject } = intent;
        match (class, subject, observation) {
            (
                class,
                Subject::Tool {
                    session,
                    message,
                    call_id,
                    ..
                },
                EffectObservation::Tool(state),
            ) => {
                expect_tool(class)?;
                self.set_tool_state(&session, message, &call_id, state)
                    .await
                    .map(|_| ())
            }
            (
                Some(EffectClass::ModelStep),
                Subject::ModelStep {
                    session,
                    run,
                    message,
                },
                EffectObservation::Assistant { parts },
            ) => self
                .append_assistant(&session, run, message, parts)
                .await
                .map(|_| ()),
            (
                Some(EffectClass::Compaction),
                Subject::Compaction { session, message },
                EffectObservation::Compaction { parts },
            ) => self
                .append_compaction(&session, message, parts)
                .await
                .map(|_| ()),
            (class, subject, observation) => {
                Err(mismatch(class, subject.name(), observation.name()))
            }
        }
    }

    /// 丢失观测：工具类写「结果未知」，原因只进日志；模型步骤与压缩不写行。
    async fn land_lost(&self, intent: EffectIntent, lost: Lost) -> Result<(), StoreError> {
        let EffectIntent { class, subject } = intent;
        match subject {
            Subject::Tool {
                session,
                message,
                call_id,
                ..
            } => {
                expect_tool(class)?;
                tracing::debug!(
                    tool_class = class.map_or("未登记的工具名", EffectClass::name),
                    message = message.get(),
                    reason = ?lost,
                    "工具调用丢失观测，结局写为未知"
                );
                self.set_tool_state(&session, message, &call_id, ToolResultState::Unknown)
                    .await
                    .map(|_| ())
            }
            Subject::ModelStep {
                session,
                run,
                message,
            } => {
                tracing::debug!(
                    session = %session.as_str(),
                    run = run.get(),
                    message = message.get(),
                    reason = %lost,
                    "模型步骤丢失观测，不写行"
                );
                Ok(())
            }
            Subject::Compaction { session, message } => {
                tracing::debug!(
                    session = %session.as_str(),
                    message = message.get(),
                    reason = %lost,
                    "压缩丢失观测，不写行"
                );
                Ok(())
            }
        }
    }
}

fn expect_tool(class: Option<EffectClass>) -> Result<(), StoreError> {
    match class {
        Some(class) if !class.is_tool() => Err(StoreError::Invariant(format!(
            "副作用类别 {} 不是工具类，不能落在 tool 节点上",
            class.name()
        ))),
        Some(_) | None => Ok(()),
    }
}

fn mismatch(class: Option<EffectClass>, subject: &str, observation: &str) -> StoreError {
    let name = class.map_or("未登记的工具名", EffectClass::name);
    StoreError::Invariant(format!(
        "副作用类别 {name} 的落点 {subject} 与观测 {observation} 不属于同一类"
    ))
}
