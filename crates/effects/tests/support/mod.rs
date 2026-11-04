//! effects 集成测试的共享夹具：一个只用于断言的类别、一个内存账本，以及各流程的
//! 短辅助。
//!
//! 每个集成测试目标是独立 crate，会单独编译本模块，因此这里允许未被该目标使用的条目。

#![expect(
    dead_code,
    clippy::expect_used,
    reason = "测试夹具用 expect 直接报告失败；本模块被每个测试目标各编译一次，未被该目标用到的条目属正常情况。"
)]

use core::future::Future;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use effects::policy::{Effect, EffectScope, Evidence, Replay, Terminal};
use effects::token::{Declared, Dispatched, Ledger, Transition, declare, dispatch};

/// 只用于断言的类别：三种重放策略与「别处覆盖」各一个取值。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Probe {
    /// 永远重发。
    Redo,
    /// 永远放弃。
    GiveUp,
    /// 已下发时按幂等分流。
    IfIdempotent,
    /// 矩阵不覆盖本类别。
    Elsewhere,
}

impl Effect for Probe {
    type Failure = &'static str;

    const ALL: &'static [Self] = &[
        Probe::Redo,
        Probe::GiveUp,
        Probe::IfIdempotent,
        Probe::Elsewhere,
    ];

    fn name(self) -> &'static str {
        match self {
            Probe::Redo => "probe_redo",
            Probe::GiveUp => "probe_give_up",
            Probe::IfIdempotent => "probe_if_idempotent",
            Probe::Elsewhere => "probe_elsewhere",
        }
    }

    fn bound(self) -> Duration {
        Duration::from_millis(50)
    }

    fn replay(self, _evidence: Evidence) -> Replay {
        match self {
            Probe::Redo | Probe::Elsewhere => Replay::Redo,
            Probe::GiveUp => Replay::GiveUp,
            Probe::IfIdempotent => Replay::IfIdempotent,
        }
    }

    fn on_timeout(self) -> Terminal<Self::Failure> {
        Terminal::Failure("probe_timeout")
    }

    fn matrix_scope(self) -> EffectScope {
        match self {
            Probe::Elsewhere => EffectScope::Elsewhere("别处的矩阵覆盖它"),
            _ => EffectScope::Covered,
        }
    }
}

/// 一次动作的意图：测试里只需要一个可比较的短标签。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Intent(pub(crate) &'static str);

/// 一次动作的观测：测试里只需要一个可比较的短标签。
pub(crate) type Observation = &'static str;

/// 账本写入失败。
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct WriteFailed;

/// 内存账本：按顺序记下每次写入；可指定某一次写入失败。
pub(crate) struct MemoryLedger {
    state: Mutex<State>,
}

struct State {
    writes: Vec<Transition<Intent, Observation>>,
    fail_at: Option<usize>,
}

impl MemoryLedger {
    pub(crate) fn new() -> Self {
        Self {
            state: Mutex::new(State {
                writes: Vec::new(),
                fail_at: None,
            }),
        }
    }

    /// 第 `attempt` 次写入（从 1 开始数）失败，且不落这一行。
    pub(crate) fn failing_at(attempt: usize) -> Self {
        let ledger = Self::new();
        ledger.state().fail_at = Some(attempt);
        ledger
    }

    pub(crate) fn writes(&self) -> Vec<Transition<Intent, Observation>> {
        self.state().writes.clone()
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Ledger for MemoryLedger {
    type Effect = Probe;
    type Intent = Intent;
    type Observation = Observation;
    type Error = WriteFailed;

    fn write(
        &self,
        transition: Transition<Self::Intent, Self::Observation>,
    ) -> impl Future<Output = Result<(), Self::Error>> {
        async move {
            let mut state = self.state();
            if state.fail_at == Some(state.writes.len() + 1) {
                return Err(WriteFailed);
            }
            state.writes.push(transition);
            Ok(())
        }
    }
}

/// 只登记，意图固定为「发送」。
pub(crate) async fn declared(ledger: &MemoryLedger) -> Declared<Probe, Intent> {
    declare(ledger, Probe::Redo, Intent("发送"))
        .await
        .expect("登记写入成功")
}

/// 登记并发货，意图固定为「发送」。
pub(crate) async fn dispatched(ledger: &MemoryLedger) -> Dispatched<Probe, Intent> {
    dispatch(ledger, declared(ledger).await)
        .await
        .expect("发货写入成功")
}
