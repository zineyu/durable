//! 模型请求的客户端超时：run 在有限时间内以 `model_timeout` 结束，与取消互不冒充（ADR 0059）。
//! 时限缩到毫秒，因此用例不必真的等；每个用例都经真实 HTTP provider 与进程内 agent 实例。

use std::time::Duration;

mod support;

use support::{Harness, Script};

/// 规格 user story A-39：模型请求超过客户端时限时，run 以 `model_timeout` 结束，队列继续。
#[tokio::test]
async fn hanging_model_request_ends_the_run_as_model_timeout() {
    let harness = Harness::start_with_model_timeout(
        agent::EngineConfig::default(),
        vec![Script::Hang, Script::Text("队列里的下一项".into())],
        Duration::from_millis(200),
    )
    .await;
    let session = harness.session("workspace").await;

    harness.submit_message(&session.id, "长时间没有响应").await;
    harness.submit_message(&session.id, "第二项").await;
    let tree = harness.wait_idle(&session.id).await;

    assert_eq!(tree.runs.len(), 2, "超时不产生额外的 run");
    assert_eq!(tree.runs[0].status, agent::RunStatus::Failed);
    let failure = tree.runs[0]
        .failure
        .as_ref()
        .expect("超时的 run 带失败说明");
    assert_eq!(
        failure.code,
        agent::RunFailureCode::ModelTimeout,
        "超时是稳定类别，不是上游错误也不是取消"
    );
    assert!(
        failure.message.contains("200 毫秒"),
        "说明里带客户端时限: {failure:?}"
    );
    let timed_out = tree.runs[0].id;
    assert!(
        tree.nodes
            .iter()
            .filter(|node| node.run_id == Some(timed_out))
            .all(|node| node.role != agent::MessageRole::Assistant),
        "超时的 run 不写 assistant 消息，因此界面不会停在执行中"
    );
    assert_eq!(
        tree.runs[1].status,
        agent::RunStatus::Succeeded,
        "超时之后队列继续"
    );
}

/// 规格 user story A-39：流已经打开而中途停滞时，客户端时限同样到点。
#[tokio::test]
async fn stalled_stream_also_ends_the_run_as_model_timeout() {
    let harness = Harness::start_with_model_timeout(
        agent::EngineConfig::default(),
        vec![Script::PartialThenHang("已产生的部分输出".into())],
        Duration::from_millis(200),
    )
    .await;
    let session = harness.session("workspace").await;

    harness.submit_message(&session.id, "中途停滞").await;
    let tree = harness.wait_idle(&session.id).await;

    assert_eq!(tree.runs[0].status, agent::RunStatus::Failed);
    let failure = tree.runs[0]
        .failure
        .as_ref()
        .expect("超时的 run 带失败说明");
    assert_eq!(failure.code, agent::RunFailureCode::ModelTimeout);
    assert!(
        tree.nodes
            .iter()
            .all(|node| node.role != agent::MessageRole::Assistant),
        "停滞的部分输出随失败的 run 一起丢弃"
    );
}

/// 规格 user story A-39：压缩请求用同一个客户端时限，超时不报告成上下文超限。
#[tokio::test]
async fn timeout_during_compaction_ends_the_run_as_model_timeout() {
    let config = agent::EngineConfig {
        context_char_limit: 300,
        ..agent::EngineConfig::default()
    };
    let harness = Harness::start_with_model_timeout(
        config,
        vec![Script::Text("答".repeat(200)), Script::Hang],
        Duration::from_millis(200),
    )
    .await;
    let session = harness.session("workspace").await;

    harness.submit_message(&session.id, "第一问").await;
    harness.wait_idle(&session.id).await;
    harness.submit_message(&session.id, "第二问").await;
    let tree = harness.wait_idle(&session.id).await;

    assert_eq!(tree.runs[1].status, agent::RunStatus::Failed);
    let failure = tree.runs[1]
        .failure
        .as_ref()
        .expect("压缩超时的 run 带失败说明");
    assert_eq!(
        failure.code,
        agent::RunFailureCode::ModelTimeout,
        "压缩超时报告副作用超时，不是上下文超限"
    );
    assert!(
        tree.nodes
            .iter()
            .all(|node| node.role != agent::MessageRole::Compaction),
        "超时不写压缩消息"
    );
}
