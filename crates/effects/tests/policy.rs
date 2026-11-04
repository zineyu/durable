//! 判定表：`settle` 把证据强度、类别的重放策略与幂等标记合成一个答案。
//! 规则逐条写死，没有兜底分支。

mod support;

use effects::policy::{Effect, Evidence, Replay, settle};
use support::Probe;

#[test]
fn declared_always_redoes() {
    let table = [
        (Probe::Redo, false),
        (Probe::GiveUp, false),
        (Probe::IfIdempotent, false),
        (Probe::GiveUp, true),
    ];

    for (effect, idempotent) in table {
        assert_eq!(
            settle(effect, Evidence::Declared, idempotent),
            Replay::Redo,
            "已登记意味着动作一定没发生：{} 只能重发",
            effect.name()
        );
    }
}

#[test]
fn dispatched_asks_idempotence_only_for_if_idempotent() {
    assert_eq!(
        settle(Probe::IfIdempotent, Evidence::Dispatched, true),
        Replay::Redo
    );
    assert_eq!(
        settle(Probe::IfIdempotent, Evidence::Dispatched, false),
        Replay::GiveUp
    );
}

#[test]
fn dispatched_returns_the_policy_verdict_otherwise() {
    let table = [(Probe::Redo, Replay::Redo), (Probe::GiveUp, Replay::GiveUp)];

    for (effect, expected) in table {
        for idempotent in [true, false] {
            assert_eq!(
                settle(effect, Evidence::Dispatched, idempotent),
                expected,
                "{} 的策略在已下发时原样生效，与幂等无关",
                effect.name()
            );
        }
    }
}
