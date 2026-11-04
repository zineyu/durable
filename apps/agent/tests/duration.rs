//! 工具与 run 的耗时：测量点在 agent 的调用点，重启后从库里读出同样的值。
//! 字段与测量规则在 ADR 0056；测试脚手架在 `tests/support`。

#![expect(clippy::panic, reason = "辅助函数用 panic 报告不符合预期的工具结局。")]

mod support;

use support::{Harness, Script};

/// 一条消息的工具耗时，按正文顺序。
async fn tool_durations(
    harness: &Harness,
    session: &agent::SessionId,
    message: agent::MessageId,
) -> Vec<(String, u64)> {
    harness
        .body(session, message)
        .await
        .tool_results()
        .map(|(call_id, state)| {
            let agent::ToolResultState::Finished { duration_ms, .. } = state else {
                panic!("工具已有耗时: {state:?}");
            };
            (call_id.to_owned(), *duration_ms)
        })
        .collect()
}

/// ADR 0056：工具耗时与 run 耗时来自单调时钟，重启后从库里读出同样的值。
#[tokio::test]
async fn records_tool_and_run_durations_across_restart() {
    let harness = Harness::start_with(
        agent::EngineConfig::default(),
        vec![
            Script::ToolCall {
                id: "call_1".into(),
                name: "bash".into(),
                arguments: r#"{"command":"sleep 0.05"}"#.into(),
            },
            Script::Text("完成".into()),
        ],
    )
    .await;
    let session = harness.session("workspace").await;
    harness.submit_message(&session.id, "计时").await;
    let tree = harness.wait_idle(&session.id).await;

    let tool = &tree.nodes[2];
    assert_eq!(tool.role, agent::MessageRole::Tool);
    let durations = tool_durations(&harness, &session.id, tool.id).await;
    assert_eq!(durations.len(), 1, "一次调用一条耗时");
    let tool_ms = durations[0].1;
    let run_ms = tree.runs[0].duration_ms.expect("已结束的 run 有耗时");
    assert!(tool_ms >= 40, "工具耗时覆盖命令本身: {tool_ms}");
    assert!(run_ms >= tool_ms, "run 耗时覆盖工具耗时: {run_ms}");

    let harness = harness.restart().await;
    let tree = harness.tree(&session.id).await;
    assert_eq!(
        tree.runs[0].duration_ms,
        Some(run_ms),
        "重启后 run 耗时不变"
    );
    assert_eq!(
        tool_durations(&harness, &session.id, tool.id).await,
        vec![("call_1".to_owned(), tool_ms)],
        "重启后工具耗时不变"
    );
}
