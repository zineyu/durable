//! 一台 host 的生命周期：状态、link、代号与取消通道。
//! 状态与 link 的唯一写入口是 [`HostLifecycle::apply`]，延迟样本由 `set_latency` 写入；
//! 事实写入与注册表在父模块；
//! link 的帧与断开原因在 `crate::link`；连接任务与退避在 `connect`；快照 DTO 在 `protocol`。
//!
//! 规格 user story R-13、R-15、R-16、R-17、R-18：状态三态显式可见，停用即断开并禁止使用，
//! 断链后按退避重连，同一 host 上的多个 session 共用一条 link。ADR 0050 记录本模块的边界。

use std::sync::Arc;

use durable::{HostKind, HostStatus};
use tokio::sync::watch;

use crate::link::{LinkClosed, LinkHandle};

/// 表里没有该 host 的运行状态时的说明。
const ABSENT_EXPLANATION: &str = "尚未连接";

/// 一次连接尝试的代号。只有最新一次尝试的结果被采纳。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Attempt(u64);

impl Attempt {
    #[must_use]
    fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

/// host 不为 `ok` 的单一原因。状态与说明都由它决定。
#[derive(Debug, Clone)]
pub(crate) enum HostFailure {
    /// 曾建成的 link 断开：executor 的退出码与标准错误尾部。
    LinkLost(LinkClosed),
    /// 断开的原因是帧协议或构建版本不符。
    ProtocolMismatch(LinkClosed),
    /// 这台 host 与本次构建不合用：远端平台没有载荷，或 host 行缺少 ssh 目标。
    Incompatible(String),
    /// 其余不可达：ssh 跑不通、安装失败、子进程拉不起来。
    Unreachable(String),
}

impl HostFailure {
    #[must_use]
    fn status(&self) -> HostStatus {
        match self {
            Self::LinkLost(_) | Self::Unreachable(_) => HostStatus::Unreachable,
            Self::ProtocolMismatch(_) | Self::Incompatible(_) => HostStatus::Incompatible,
        }
    }

    #[must_use]
    fn explanation(&self) -> String {
        match self {
            Self::LinkLost(close) => close.detail(),
            Self::ProtocolMismatch(close) => format!("帧协议版本不符（{}）", close.detail()),
            Self::Incompatible(text) | Self::Unreachable(text) => text.clone(),
        }
    }

    /// 把一条断开的 link 归到原因上。退出码 3 表示版本不符。
    #[must_use]
    fn of_close(close: LinkClosed) -> Self {
        if close.protocol_mismatch() {
            Self::ProtocolMismatch(close)
        } else {
            Self::LinkLost(close)
        }
    }
}

/// 一台 host 对外的状态。`Ok` 一定带着那条能用的 link。
#[derive(Clone)]
pub(crate) enum HostState {
    /// link 可用。
    Ok(Arc<LinkHandle>),
    /// 正在建立 link。
    Connecting,
    /// 从来没有连上过，或运行状态还没建立。
    NeverConnected,
    /// host 表的 `enabled` 为假。
    Disabled,
    /// 最近一次尝试或最近一条 link 失败。
    Failed(HostFailure),
}

impl HostState {
    #[must_use]
    pub(crate) fn status(&self) -> HostStatus {
        match self {
            Self::Ok(_) => HostStatus::Ok,
            Self::Connecting | Self::NeverConnected | Self::Disabled => HostStatus::Unreachable,
            Self::Failed(failure) => failure.status(),
        }
    }

    /// 状态的说明。`ok` 时为空。
    #[must_use]
    pub(crate) fn explanation(&self) -> Option<String> {
        match self {
            Self::Ok(_) => None,
            Self::Connecting => Some("正在连接".to_owned()),
            Self::NeverConnected => Some(ABSENT_EXPLANATION.to_owned()),
            Self::Disabled => Some("host 已停用".to_owned()),
            Self::Failed(failure) => Some(failure.explanation()),
        }
    }

    #[must_use]
    pub(crate) fn link(&self) -> Option<&Arc<LinkHandle>> {
        match self {
            Self::Ok(link) => Some(link),
            Self::Connecting | Self::NeverConnected | Self::Disabled | Self::Failed(_) => None,
        }
    }

    /// 表里没有该 host 的运行状态时的说明。
    #[must_use]
    pub(crate) fn absent_explanation() -> String {
        ABSENT_EXPLANATION.to_owned()
    }
}

/// 生命周期接受的事件。连接任务只报告一次尝试的结果，不写状态。
pub(crate) enum HostEvent {
    /// 按 host 行重建连接：丢弃当前 link、作废旧代号、进入「正在连接」。
    Connecting,
    /// 一次尝试建成了 link。
    Opened(Attempt, Arc<LinkHandle>),
    /// 一次尝试失败。
    AttemptFailed(Attempt, HostFailure),
    /// 一条曾建成的 link 断开。
    Close(Attempt, LinkClosed),
    /// host 表里的 `enabled` 变成假。
    Disabled,
}

/// 一条事件的结果。
pub(crate) struct Applied {
    /// 事件被采纳。代号与当前不一致的事件不被采纳，也不改任何字段。
    pub(crate) accepted: bool,
    /// 采纳之后对外的状态或说明变了，registry 据此写一条事实。
    pub(crate) changed: Option<HostState>,
    /// 本次事件签发的新代号与它的取消接收端。只有 `Connecting` 给出。
    pub(crate) attempt: Option<(Attempt, watch::Receiver<()>)>,
}

impl Applied {
    fn of(changed: Option<HostState>) -> Self {
        Self {
            accepted: true,
            changed,
            attempt: None,
        }
    }

    fn stale() -> Self {
        Self {
            accepted: false,
            changed: None,
            attempt: None,
        }
    }
}

/// 一台 host 的进程内状态。不落库，只经事实事件对外。
pub(crate) struct HostLifecycle {
    /// 连接方式。只有 ssh 的建立要探测与安装，耗时可见，因此先给出「正在连接」。
    kind: HostKind,
    state: HostState,
    latency_ms: Option<u64>,
    /// 当前尝试的代号。每次 `Connecting` 与 `Disabled` 递增，旧尝试的迟到事件因此失效。
    attempt: Attempt,
    /// ssh host 的重连任务。丢弃它即让任务退出：重建、停用与删除都这样做。
    cancel: Option<watch::Sender<()>>,
}

impl HostLifecycle {
    /// 刚登记、还没连过的 host。
    #[must_use]
    pub(crate) fn new(kind: HostKind) -> Self {
        Self {
            kind,
            state: HostState::NeverConnected,
            latency_ms: None,
            attempt: Attempt(0),
            cancel: None,
        }
    }

    #[must_use]
    pub(crate) fn state(&self) -> &HostState {
        &self.state
    }

    /// 最近一次心跳的往返毫秒数。还没收到过应答时为 `None`。
    #[must_use]
    pub(crate) fn latency_ms(&self) -> Option<u64> {
        self.latency_ms
    }

    /// 当前尝试的代号。心跳带着它写延迟，旧尝试的样本因此不会落到新的 link 上。
    #[must_use]
    pub(crate) fn attempt(&self) -> Attempt {
        self.attempt
    }

    /// 应用一条事件，返回是否采纳与状态是否变化。
    pub(crate) fn apply(&mut self, event: HostEvent) -> Applied {
        match event {
            HostEvent::Connecting => {
                let (cancel, receiver) = watch::channel(());
                self.cancel = Some(cancel);
                self.attempt = self.attempt.next();
                let attempt = self.attempt;
                self.latency_ms = None;
                // 本机建链是本地动作，状态与说明留到打开或失败时再写，因此不产生过渡事实。
                let changed = match self.kind {
                    HostKind::Local => None,
                    HostKind::Ssh => self.replace(HostState::Connecting),
                };
                Applied {
                    accepted: true,
                    changed,
                    attempt: Some((attempt, receiver)),
                }
            }
            HostEvent::Opened(attempt, link) => {
                if attempt != self.attempt {
                    return Applied::stale();
                }
                self.latency_ms = None;
                Applied::of(self.replace(HostState::Ok(link)))
            }
            HostEvent::AttemptFailed(attempt, failure) => {
                if attempt != self.attempt {
                    return Applied::stale();
                }
                self.latency_ms = None;
                Applied::of(self.replace(HostState::Failed(failure)))
            }
            HostEvent::Close(attempt, close) => {
                if attempt != self.attempt {
                    return Applied::stale();
                }
                self.latency_ms = None;
                Applied::of(self.replace(HostState::Failed(HostFailure::of_close(close))))
            }
            HostEvent::Disabled => {
                self.cancel = None;
                self.attempt = self.attempt.next();
                self.latency_ms = None;
                Applied::of(self.replace(HostState::Disabled))
            }
        }
    }

    /// 写一个延迟样本。只有当前尝试且 link 可用时采纳。
    pub(crate) fn set_latency(&mut self, attempt: Attempt, latency_ms: u64) {
        if attempt == self.attempt && matches!(self.state, HostState::Ok(_)) {
            self.latency_ms = Some(latency_ms);
        }
    }

    /// 换掉状态，并回答对外可见的状态与说明是否变化。
    ///
    /// 按状态与说明比较，不按枚举相等比较：两条不同的 link 都是 `ok`，不构成变化。
    fn replace(&mut self, next: HostState) -> Option<HostState> {
        let changed =
            self.state.status() != next.status() || self.state.explanation() != next.explanation();
        self.state = next;
        if changed {
            Some(self.state.clone())
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use durable::{HostKind, HostStatus};

    use super::{HostEvent, HostFailure, HostLifecycle, HostState};
    use crate::link::{INCOMPATIBLE_EXIT_CODE, LinkClosed, LinkHandle};

    fn ssh() -> HostLifecycle {
        HostLifecycle::new(HostKind::Ssh)
    }

    fn local() -> HostLifecycle {
        HostLifecycle::new(HostKind::Local)
    }

    fn opened_link() -> Arc<LinkHandle> {
        Arc::new(LinkHandle::detached())
    }

    fn close_with(code: i32) -> LinkClosed {
        LinkClosed {
            exit_code: Some(code),
            stderr_tail: "stdout 已关闭".to_owned(),
        }
    }

    fn connect(
        lifecycle: &mut HostLifecycle,
    ) -> (super::Attempt, tokio::sync::watch::Receiver<()>) {
        let applied = lifecycle.apply(HostEvent::Connecting);
        assert!(applied.accepted, "重建连接总被采纳");
        applied.attempt.expect("重建连接签发了代号")
    }

    #[test]
    fn a_fresh_host_is_not_connected_yet() {
        let lifecycle = local();
        assert_eq!(lifecycle.state().status(), HostStatus::Unreachable);
        assert_eq!(lifecycle.state().explanation().as_deref(), Some("尚未连接"));
        assert!(lifecycle.state().link().is_none(), "没有 link");
    }

    #[test]
    fn a_local_host_has_no_connecting_state() {
        let mut lifecycle = local();
        let applied = lifecycle.apply(HostEvent::Connecting);
        assert!(applied.accepted);
        assert!(applied.changed.is_none(), "本机建链不写过渡状态");
        assert_eq!(lifecycle.state().explanation().as_deref(), Some("尚未连接"));
    }

    #[test]
    fn opening_a_link_makes_the_host_usable() {
        let mut lifecycle = ssh();
        let (attempt, _cancel) = connect(&mut lifecycle);
        let applied = lifecycle.apply(HostEvent::Opened(attempt, opened_link()));
        assert!(applied.accepted);
        assert!(
            applied.changed.is_some(),
            "从「正在连接」变成 ok 是一条事实"
        );
        assert_eq!(lifecycle.state().status(), HostStatus::Ok);
        assert!(lifecycle.state().explanation().is_none(), "ok 没有说明");
        assert!(lifecycle.state().link().is_some(), "ok 带着 link");
    }

    #[test]
    fn a_second_open_on_the_same_attempt_is_not_a_new_fact() {
        let mut lifecycle = ssh();
        let (attempt, _cancel) = connect(&mut lifecycle);
        lifecycle.apply(HostEvent::Opened(attempt, opened_link()));
        let applied = lifecycle.apply(HostEvent::Opened(attempt, opened_link()));
        assert!(applied.accepted);
        assert!(applied.changed.is_none(), "ok 换成另一条 link 不是状态变化");
    }

    #[test]
    fn a_late_open_after_disable_is_dropped() {
        let mut lifecycle = ssh();
        let (stale, _cancel) = connect(&mut lifecycle);
        lifecycle.apply(HostEvent::Disabled);
        let applied = lifecycle.apply(HostEvent::Opened(stale, opened_link()));
        assert!(!applied.accepted, "停用之后到达的打开被丢弃");
        assert!(matches!(lifecycle.state(), HostState::Disabled));
        assert!(lifecycle.state().link().is_none(), "停用的 host 没有 link");
    }

    #[test]
    fn a_close_from_a_replaced_link_is_dropped() {
        let mut lifecycle = ssh();
        let (first, _first_cancel) = connect(&mut lifecycle);
        lifecycle.apply(HostEvent::Opened(first, opened_link()));
        let (second, _second_cancel) = connect(&mut lifecycle);
        let applied = lifecycle.apply(HostEvent::Close(first, close_with(1)));
        assert!(!applied.accepted);
        assert_eq!(
            lifecycle.state().status(),
            HostStatus::Unreachable,
            "仍在连接中"
        );
        assert_eq!(lifecycle.state().explanation().as_deref(), Some("正在连接"));
        assert_eq!(lifecycle.attempt(), second, "代号没有被旧关闭通知改动");
    }

    #[test]
    fn a_protocol_mismatch_becomes_incompatible() {
        let mut lifecycle = ssh();
        let (attempt, _cancel) = connect(&mut lifecycle);
        lifecycle.apply(HostEvent::Opened(attempt, opened_link()));
        let applied = lifecycle.apply(HostEvent::Close(
            attempt,
            close_with(INCOMPATIBLE_EXIT_CODE),
        ));
        assert!(applied.accepted);
        assert_eq!(lifecycle.state().status(), HostStatus::Incompatible);
        let explanation = lifecycle.state().explanation().unwrap_or_default();
        assert!(
            explanation.starts_with("帧协议版本不符"),
            "说明应指出协议不符：{explanation}"
        );
        assert!(lifecycle.state().link().is_none());
    }

    #[test]
    fn an_ordinary_exit_is_unreachable() {
        let mut lifecycle = ssh();
        let (attempt, _cancel) = connect(&mut lifecycle);
        lifecycle.apply(HostEvent::Opened(attempt, opened_link()));
        lifecycle.apply(HostEvent::Close(attempt, close_with(1)));
        assert_eq!(lifecycle.state().status(), HostStatus::Unreachable);
        let explanation = lifecycle.state().explanation().unwrap_or_default();
        assert!(
            explanation.contains("退出码 1"),
            "说明应带退出码：{explanation}"
        );
    }

    #[test]
    fn a_failed_attempt_keeps_its_reason() {
        let mut lifecycle = ssh();
        let (attempt, _cancel) = connect(&mut lifecycle);
        let applied = lifecycle.apply(HostEvent::AttemptFailed(
            attempt,
            HostFailure::Incompatible("不支持远端系统".to_owned()),
        ));
        assert!(applied.accepted);
        assert_eq!(lifecycle.state().status(), HostStatus::Incompatible);
        assert_eq!(
            lifecycle.state().explanation().as_deref(),
            Some("不支持远端系统")
        );
    }

    #[test]
    fn disabling_twice_writes_one_fact() {
        let mut lifecycle = ssh();
        assert!(lifecycle.apply(HostEvent::Disabled).changed.is_some());
        assert!(
            lifecycle.apply(HostEvent::Disabled).changed.is_none(),
            "状态与说明都没变"
        );
    }

    #[test]
    fn a_stale_latency_sample_does_not_land_on_the_new_link() {
        let mut lifecycle = ssh();
        let (first, _first_cancel) = connect(&mut lifecycle);
        lifecycle.apply(HostEvent::Opened(first, opened_link()));
        let (second, _second_cancel) = connect(&mut lifecycle);
        lifecycle.apply(HostEvent::Opened(second, opened_link()));
        lifecycle.set_latency(first, 5);
        assert_eq!(lifecycle.latency_ms(), None, "旧尝试的样本不落到新 link 上");
        lifecycle.set_latency(second, 6);
        assert_eq!(lifecycle.latency_ms(), Some(6));
    }

    #[test]
    fn a_retry_with_another_reason_writes_a_new_fact() {
        let mut lifecycle = ssh();
        let (attempt, _cancel) = connect(&mut lifecycle);
        lifecycle.apply(HostEvent::AttemptFailed(
            attempt,
            HostFailure::Unreachable("连接超时".to_owned()),
        ));
        let applied = lifecycle.apply(HostEvent::AttemptFailed(
            attempt,
            HostFailure::Unreachable("密码错误".to_owned()),
        ));
        assert!(applied.accepted, "同一次尝试的再次失败仍被采纳");
        assert!(applied.changed.is_some(), "说明变了也是一条事实");
        assert_eq!(lifecycle.state().explanation().as_deref(), Some("密码错误"));
    }

    #[test]
    fn latency_lands_only_on_the_current_attempt() {
        let mut lifecycle = ssh();
        let (attempt, _cancel) = connect(&mut lifecycle);
        lifecycle.apply(HostEvent::Opened(attempt, opened_link()));
        lifecycle.set_latency(attempt, 7);
        assert_eq!(lifecycle.latency_ms(), Some(7));
        let (next, _next_cancel) = connect(&mut lifecycle);
        assert_eq!(lifecycle.latency_ms(), None, "新的尝试清掉旧样本");
        lifecycle.set_latency(attempt, 9);
        assert_eq!(lifecycle.latency_ms(), None, "旧尝试的样本被丢弃");
        lifecycle.set_latency(next, 11);
        assert_eq!(lifecycle.latency_ms(), None, "非 ok 的 host 不记延迟");
    }
}
