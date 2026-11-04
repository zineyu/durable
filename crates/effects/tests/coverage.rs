//! 覆盖率：声明要覆盖的类别必须真的出现过，否则名字被报出来。

use effects::coverage::Coverage;
use effects::policy::{Effect, EffectScope};

mod support;

use support::Probe;

/// 把全部「声明覆盖」的类别各记一次。
fn cover_everything(coverage: &Coverage<Probe>) {
    for effect in Probe::ALL {
        if effect.matrix_scope() == EffectScope::Covered {
            coverage.record(*effect);
        }
    }
}

#[test]
fn gaps_are_empty_once_every_covered_category_happened() {
    let coverage = Coverage::new();

    cover_everything(&coverage);

    assert!(
        coverage.gaps().is_empty(),
        "全部声明覆盖的类别都出现过，缺口应为空"
    );
}

#[test]
fn a_covered_category_that_never_happened_is_named() {
    let coverage = Coverage::new();
    coverage.record(Probe::Redo);

    assert_eq!(
        coverage.gaps(),
        vec!["probe_give_up", "probe_if_idempotent"]
    );
}

#[test]
fn an_elsewhere_category_never_enters_the_gap_list() {
    let coverage = Coverage::new();
    coverage.record(Probe::Redo);
    coverage.record(Probe::GiveUp);
    coverage.record(Probe::IfIdempotent);

    assert!(
        !coverage.gaps().contains(&"probe_elsewhere"),
        "别处覆盖的类别不是缺口"
    );
}

#[test]
fn declared_keeps_each_category_once_in_first_occurrence_order() {
    let coverage = Coverage::new();
    coverage.record(Probe::GiveUp);
    coverage.record(Probe::Redo);
    coverage.record(Probe::GiveUp);

    assert_eq!(coverage.declared(), vec![Probe::GiveUp, Probe::Redo]);
}
