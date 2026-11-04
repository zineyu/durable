//! 许可证与流程：先写库，再发出许可证；执行只接受已下发的许可证。
//! 这是 ADR 0037「工具节点先入树再执行」的同一条规矩，推广到任意一类副作用；
//! 账本怎么写、结局在业务上是什么意思都归使用方的 [`Ledger`]，策略表在 `policy`。

use core::future::Future;

use crate::policy::Effect;

/// 账本的一次写入。证据强度由变体本身表示。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Transition<I, O> {
    /// 已登记：动作一定没有发生。
    Declared(I),
    /// 已下发：动作可能已经发生。
    Dispatched(I),
    /// 有观测：动作做完了，附观测。
    Observed(I, O),
    /// 丢失观测：动作可能已经发生，但没有得到结果。
    Lost(I, Lost),
}

/// 丢失观测的原因。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Lost {
    /// host 断开，附原文。
    HostGone(String),
    /// 类别上界到点。
    Timeout,
    /// 已登记但决定不执行。
    Cancelled,
}

/// 一次动作的结论：拿到观测，或丢失观测。
#[derive(Clone, Debug, PartialEq, Eq)]
#[must_use]
pub enum Outcome<O> {
    /// 拿到观测。
    Seen(O),
    /// 丢失观测，附原因。
    Lost(Lost),
}

/// 使用方的账本。库只调用 [`write`](Ledger::write)，不规定它落到哪里。
pub trait Ledger {
    /// 本次动作的类别。
    type Effect: Effect;
    /// 动作的意图。写库与返回都要用它，因此必须能复制一份。
    type Intent: Clone;
    /// 动作的观测。
    type Observation;
    /// 使用方的错误类型。库只做传播，不定义错误。
    type Error;

    /// 写入一行。
    ///
    /// 返回类型写成 `impl Future` 而不是 `async fn`：同样的「不装箱、不做 `dyn`」，
    /// 但不必给公开 trait 的 `async fn` 加抑制。`Send` 由调用点自己检查。
    ///
    /// # Errors
    ///
    /// 返回使用方的错误。库把它原样向上传播，并停止发放许可证。
    fn write(
        &self,
        transition: Transition<Self::Intent, Self::Observation>,
    ) -> impl Future<Output = Result<(), Self::Error>>;
}

/// 密封标记：许可证的字段私有，且没有构造入口，因此外部无法凭空造出一张。
struct Seal;

/// 已登记的许可证：动作一定没有发生。
#[must_use]
pub struct Declared<E, I> {
    effect: E,
    intent: I,
    _seal: Seal,
}

/// 已下发的许可证：动作可能已经发生。它不实现 [`Clone`]，一次下发只产出一个结局。
#[must_use]
pub struct Dispatched<E, I> {
    effect: E,
    intent: I,
    _seal: Seal,
}

impl<E: Effect, I> Declared<E, I> {
    /// 本次动作的意图。
    #[must_use]
    pub fn intent(&self) -> &I {
        &self.intent
    }
}

impl<E: Effect, I> Dispatched<E, I> {
    /// 本次动作的意图。
    #[must_use]
    pub fn intent(&self) -> &I {
        &self.intent
    }
}

/// 登记：先写库，写成功才发出许可证。
///
/// # Errors
///
/// 账本写入失败时返回使用方的错误，且不发出许可证。
pub async fn declare<L: Ledger>(
    ledger: &L,
    effect: L::Effect,
    intent: L::Intent,
) -> Result<Declared<L::Effect, L::Intent>, L::Error> {
    ledger.write(Transition::Declared(intent.clone())).await?;
    Ok(Declared {
        effect,
        intent,
        _seal: Seal,
    })
}

/// 发货：先写库，写成功才把许可证升级为已下发。
///
/// # Errors
///
/// 账本写入失败时返回使用方的错误，且不发出已下发的许可证。
pub async fn dispatch<L: Ledger>(
    ledger: &L,
    declared: Declared<L::Effect, L::Intent>,
) -> Result<Dispatched<L::Effect, L::Intent>, L::Error> {
    ledger
        .write(Transition::Dispatched(declared.intent.clone()))
        .await?;
    Ok(Dispatched {
        effect: declared.effect,
        intent: declared.intent,
        _seal: Seal,
    })
}

/// 执行：把工作体包在类别上界内，先把结论写库，再把结论交给调用方。
///
/// 工作体拿到已下发的许可证，因此它只能在这一步之后执行。上界到点按
/// [`Lost::Timeout`] 收尾，工作体给出的 [`Lost`] 原样落库并原样返回。
///
/// # Errors
///
/// 账本写入失败时返回使用方的错误。此时证据停在「已下发」：使用方不得把落库失败
/// 当成动作没发生。
pub async fn perform<L, F, Fut>(
    ledger: &L,
    dispatched: Dispatched<L::Effect, L::Intent>,
    work: F,
) -> Result<Outcome<L::Observation>, L::Error>
where
    L: Ledger,
    L::Observation: Clone,
    F: FnOnce(&Dispatched<L::Effect, L::Intent>) -> Fut,
    Fut: Future<Output = Outcome<L::Observation>>,
{
    let bound = dispatched.effect.bound();
    let intent = dispatched.intent.clone();
    let outcome = tokio::time::timeout(bound, work(&dispatched))
        .await
        .unwrap_or(Outcome::Lost(Lost::Timeout));

    match &outcome {
        Outcome::Seen(observation) => {
            ledger
                .write(Transition::Observed(intent, observation.clone()))
                .await?;
        }
        Outcome::Lost(lost) => {
            ledger.write(Transition::Lost(intent, lost.clone())).await?;
        }
    }

    Ok(outcome)
}

/// 弃：已登记但决定不执行。批次中途取消、断链后剩余的调用走这里。
///
/// # Errors
///
/// 账本写入失败时返回使用方的错误，且不给出结论。
pub async fn abandon<L: Ledger>(
    ledger: &L,
    declared: Declared<L::Effect, L::Intent>,
    lost: Lost,
) -> Result<Outcome<L::Observation>, L::Error> {
    ledger
        .write(Transition::Lost(declared.intent, lost.clone()))
        .await?;
    Ok(Outcome::Lost(lost))
}
