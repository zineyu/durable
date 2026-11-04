//! run 的存储失败：内核的 `StoreError` 记为 `storage_failed`，不冒充上游或上下文失败。
//! 这里的失败由直连数据库写坏一行或拒绝写入制造，不需要生产代码里的注入点。

mod support;

use support::{Harness, Script, corrupt_message_parts, reject_compaction_writes};

/// ADR 0043：读取存储中的消息失败时 run 记为存储失败，不冒充上游失败。
#[tokio::test]
async fn fails_run_as_storage_failure_when_stored_message_is_corrupted() {
    let harness = Harness::start_with(
        agent::EngineConfig::default(),
        vec![
            Script::Text("第一答".into()),
            Script::Text("不会用到".into()),
        ],
    )
    .await;
    let session = harness.session("workspace").await;

    harness.submit_message(&session.id, "第一问").await;
    harness.wait_idle(&session.id).await;
    corrupt_message_parts(&harness.database).await;

    harness.submit_message(&session.id, "第二问").await;
    let tree = harness.wait_idle(&session.id).await;

    let failure = tree.runs[1].failure.as_ref().expect("失败原因");
    assert_eq!(failure.code, agent::RunFailureCode::StorageFailed);
    // 极端断言按验收清单要求保留：存储失败不得再冒充上游失败。
    assert_ne!(failure.code, agent::RunFailureCode::ModelUpstream);
    assert!(
        failure.message.contains("message_part.body"),
        "失败说明要写明损坏的字段: {}",
        failure.message
    );
    assert_eq!(tree.runs[1].status, agent::RunStatus::Failed);
}

/// ADR 0043：压缩写库失败记为存储失败，模型侧失败仍是上下文超限。
#[tokio::test]
async fn fails_run_as_storage_failure_when_compaction_cannot_be_written() {
    let config = agent::EngineConfig {
        context_char_limit: 300,
        ..agent::EngineConfig::default()
    };
    let harness = Harness::start_with(
        config,
        vec![
            Script::Text("答".repeat(200)),
            Script::Text("摘要".into()),
            Script::Text("不会用到".into()),
        ],
    )
    .await;
    let session = harness.session("workspace").await;

    harness.submit_message(&session.id, "第一问").await;
    harness.wait_idle(&session.id).await;
    reject_compaction_writes(&harness.database).await;

    harness.submit_message(&session.id, "第二问").await;
    let tree = harness.wait_idle(&session.id).await;

    let failure = tree.runs[1].failure.as_ref().expect("失败原因");
    assert_eq!(failure.code, agent::RunFailureCode::StorageFailed);
    // 极端断言按验收清单要求保留：压缩的写库失败不得冒充上下文超限。
    assert_ne!(failure.code, agent::RunFailureCode::ContextLimitExceeded);
    assert!(
        failure.message.contains("测试注入的写入失败"),
        "失败说明要带存储错误的原文: {}",
        failure.message
    );
}
