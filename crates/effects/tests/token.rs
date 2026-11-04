//! 许可证的流程：每次升级都先写库，写成功才发出下一张许可证。

mod support;

use std::future::pending;
use std::time::Instant;

use effects::policy::Effect;
use effects::token::{Lost, Outcome, Transition, abandon, declare, perform};
use support::{Intent, MemoryLedger, Probe, WriteFailed, declared, dispatched};

#[tokio::test]
async fn declare_writes_declared_evidence_before_issuing_the_token() {
    let ledger = MemoryLedger::new();

    let token = declare(&ledger, Probe::Redo, Intent("发送"))
        .await
        .expect("登记写入成功");

    assert_eq!(token.intent(), &Intent("发送"));
    assert_eq!(ledger.writes(), vec![Transition::Declared(Intent("发送"))]);
}

#[tokio::test]
async fn dispatch_upgrades_the_evidence_and_keeps_the_intent() {
    let ledger = MemoryLedger::new();

    let token = dispatched(&ledger).await;

    assert_eq!(token.intent(), &Intent("发送"));
    assert_eq!(
        ledger.writes(),
        vec![
            Transition::Declared(Intent("发送")),
            Transition::Dispatched(Intent("发送")),
        ]
    );
}

#[tokio::test]
async fn perform_writes_the_observation_then_returns_it() {
    let ledger = MemoryLedger::new();

    let outcome = perform(&ledger, dispatched(&ledger).await, |token| {
        let intent = token.intent().clone();
        async move { Outcome::Seen(intent.0) }
    })
    .await
    .expect("结局写入成功");

    assert_eq!(outcome, Outcome::Seen("发送"));
    assert_eq!(
        ledger.writes(),
        vec![
            Transition::Declared(Intent("发送")),
            Transition::Dispatched(Intent("发送")),
            Transition::Observed(Intent("发送"), "发送"),
        ],
        "正常路径是三次写入，顺序固定"
    );
}

#[tokio::test]
async fn perform_stops_at_the_effect_bound_and_writes_a_timeout() {
    let ledger = MemoryLedger::new();
    let started = Instant::now();

    let outcome = perform(&ledger, dispatched(&ledger).await, |_token| pending())
        .await
        .expect("超时结论写入成功");

    assert_eq!(outcome, Outcome::Lost(Lost::Timeout));
    assert!(
        started.elapsed() >= Probe::Redo.bound(),
        "上界必须来自类别的 bound()，而不是工作体自己"
    );
    assert_eq!(
        ledger.writes().last(),
        Some(&Transition::Lost(Intent("发送"), Lost::Timeout))
    );
}

#[tokio::test]
async fn perform_forwards_an_unobserved_ending_as_is() {
    let ledger = MemoryLedger::new();
    let gone = Lost::HostGone("链接断开".to_owned());

    let outcome = perform(&ledger, dispatched(&ledger).await, |_token| {
        let lost = gone.clone();
        async move { Outcome::Lost(lost) }
    })
    .await
    .expect("断链结论写入成功");

    assert_eq!(outcome, Outcome::Lost(gone.clone()));
    assert_eq!(
        ledger.writes().last(),
        Some(&Transition::Lost(Intent("发送"), gone))
    );
}

#[tokio::test]
async fn abandon_writes_the_lost_evidence_for_an_undispatched_call() {
    let ledger = MemoryLedger::new();

    let outcome = abandon(&ledger, declared(&ledger).await, Lost::Cancelled)
        .await
        .expect("放弃写入成功");

    assert_eq!(outcome, Outcome::Lost(Lost::Cancelled));
    assert_eq!(
        ledger.writes(),
        vec![
            Transition::Declared(Intent("发送")),
            Transition::Lost(Intent("发送"), Lost::Cancelled),
        ]
    );
}

#[tokio::test]
async fn a_failed_perform_write_propagates_and_stays_dispatched() {
    let ledger = MemoryLedger::failing_at(3);

    let result = perform(&ledger, dispatched(&ledger).await, |token| {
        let intent = token.intent().clone();
        async move { Outcome::Seen(intent.0) }
    })
    .await;

    assert_eq!(result, Err(WriteFailed));
    assert_eq!(
        ledger.writes(),
        vec![
            Transition::Declared(Intent("发送")),
            Transition::Dispatched(Intent("发送")),
        ],
        "落库失败时证据停在已下发，不能回退成已登记"
    );
}

#[tokio::test]
async fn a_failed_declare_issues_no_token() {
    let ledger = MemoryLedger::failing_at(1);

    let result = declare(&ledger, Probe::Redo, Intent("发送")).await;

    assert!(result.is_err(), "写入失败时不得发出许可证");
    assert!(ledger.writes().is_empty(), "失败的写入不落行");
}
