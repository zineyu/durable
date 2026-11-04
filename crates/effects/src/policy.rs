//! 副作用类别表：一类副作用必须回答的问题，以及把策略与证据合成判定的 [`settle`]。
//! 有哪些类别、上界填多少、失败码长什么样都归使用方；许可证与流程在 `token`，
//! 覆盖率在 `coverage`。

use core::fmt::Debug;
use core::time::Duration;

/// 一类副作用。五个成员加起来是「新增一类时必须有人表态」的强制点。
pub trait Effect: Copy + Eq + Debug + 'static {
    /// 超时按哪种失败收尾。库因此不必认识使用方的失败码。
    type Failure: Copy;

    /// 唯一的类别清单。覆盖率与矩阵都遍历它。
    ///
    /// 这里是全套里唯一靠测试而不是编译器守住的清单：少写一行编译器不会出声。
    const ALL: &'static [Self];

    /// 稳定字面量，不用 `Debug` 的格式串。日志、告警与测试失败信息都据它说话。
    fn name(self) -> &'static str;

    /// 一次对外动作的截止时间，liveness 的唯一来源。
    ///
    /// 使用方的重试循环（限流退避）应在工作体内部，因此上界必须大于最坏重试预算。
    fn bound(self) -> Duration;

    /// 已经拿到这个强度的证据时，重启后该怎么办。
    fn replay(self, evidence: Evidence) -> Replay;

    /// 超时的收尾。超时绝不静默：`Failure` 由使用方收尾，`Unrelated` 必须写出理由。
    fn on_timeout(self) -> Terminal<Self::Failure>;

    /// 矩阵必须覆盖，还是别处覆盖。`Elsewhere` 必须给出理由。
    fn matrix_scope(self) -> EffectScope;
}

/// 证据强度：动作确定没发生，还是可能已经发生。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Evidence {
    /// 已登记、未下发。动作一定没有发生。
    Declared,
    /// 已下发。动作可能已经发生。
    Dispatched,
}

/// 重启后的处置。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Replay {
    /// 重发。
    Redo,
    /// 放弃：不再重发。
    GiveUp,
    /// 已下发时按幂等分流。
    IfIdempotent,
}

/// 超时后的收尾。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Terminal<F> {
    /// 按这个失败码收尾。
    Failure(F),
    /// 不归本类别管，附理由。
    Unrelated(&'static str),
}

/// 一类副作用在矩阵里的位置。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectScope {
    /// 承诺：矩阵必须真的走到。
    Covered,
    /// 别处覆盖。必须给出理由，「没覆盖」与「忘了覆盖」在结果上一样。
    Elsewhere(&'static str),
}

/// 把证据强度、类别的重放策略与幂等标记合成重启后的处置。
///
/// 规则逐条写死：已登记一定没发生，因此重发；已下发按类别策略，只有
/// [`Replay::IfIdempotent`] 才看幂等标记。`IfIdempotent` 出现在已登记分支属于
/// 使用方的表错误，本函数不看它。
#[must_use]
pub fn settle<E: Effect>(effect: E, evidence: Evidence, idempotent: bool) -> Replay {
    match evidence {
        Evidence::Declared => Replay::Redo,
        Evidence::Dispatched => match effect.replay(evidence) {
            Replay::Redo => Replay::Redo,
            Replay::GiveUp => Replay::GiveUp,
            Replay::IfIdempotent => {
                if idempotent {
                    Replay::Redo
                } else {
                    Replay::GiveUp
                }
            }
        },
    }
}
