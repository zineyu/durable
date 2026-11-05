//! 副作用类别表：一类副作用必须回答的问题，以及把策略与证据合成判定的 [`settle`]。
//! 有哪些类别、重放策略长什么样都归使用方；许可证与流程在 `token`，覆盖率在
//! `coverage`。超时不在这张表里：它归各介质层，见 ADR 0059。

use core::fmt::Debug;

/// 一类副作用。四个成员加上 `ALL` 是「新增一类时必须有人表态」的强制点。
pub trait Effect: Copy + Eq + Debug + 'static {
    /// 唯一的类别清单。覆盖率与矩阵都遍历它。
    ///
    /// 清单与变体集合的一致性由 [`effect_table!`](crate::effect_table) 保证：一份声明同时
    /// 生成枚举与清单，因此不存在「清单落后于变体」这个形状（ADR 0062）。手写实现要自己担
    /// 这个风险。
    const ALL: &'static [Self];

    /// 稳定字面量，不用 `Debug` 的格式串。日志、告警与测试失败信息都据它说话。
    fn name(self) -> &'static str;

    /// 已经拿到这个强度的证据时，重启后该怎么办。
    fn replay(self, evidence: Evidence) -> Replay;

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
