//! 规格 user story R-36…R-38：link 断开时正在等结果的 run 以 `host_unreachable` 结束，
//! 在途工具调用记为「结果未知」，队列继续执行下一项。
//!
//! 断链用杀掉本地 executor 子进程模拟；进程组在 executor 被强杀时不保证清理，见 ADR 0048。

use std::time::Duration;

mod support;

use support::provider::ToolCallSpec;
use support::{Harness, Script};

#[tokio::test]
async fn executor_killed_mid_tool_fails_run_and_continues_queue() {
    let harness = Harness::start_with(
        agent::EngineConfig::default(),
        vec![
            Script::ToolCalls(vec![ToolCallSpec::new(
                "call_slow",
                "bash",
                r#"{"command":"touch started.txt; sleep 30"}"#,
            )]),
            Script::Text("继续".into()),
        ],
    )
    .await;
    let session = harness.session("workspace").await;
    let marker = harness.workspace_path("workspace").join("started.txt");

    harness.submit_message(&session.id, "长时间命令").await;
    for _ in 0..500 {
        if marker.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(marker.exists(), "bash 已开始执行");

    harness.submit_message(&session.id, "继续").await;
    support::kill_process(harness.executor_pid());

    let tree = harness.wait_idle(&session.id).await;
    assert_eq!(tree.runs.len(), 2, "断链后队列继续执行下一项");
    let failure = tree.runs[0]
        .failure
        .as_ref()
        .expect("失败的 run 带失败说明");
    assert_eq!(
        tree.runs[0].status,
        agent::RunStatus::Failed,
        "断链让 run 失败"
    );
    assert_eq!(failure.code, agent::RunFailureCode::HostUnreachable);
    assert!(!failure.message.is_empty(), "失败说明非空");
    assert_eq!(tree.runs[1].status, agent::RunStatus::Succeeded);

    let tool = tree
        .nodes
        .iter()
        .find(|node| node.role == agent::MessageRole::Tool)
        .expect("在途调用有 tool 节点");
    let states: Vec<agent::ToolResultState> = harness
        .body(&session.id, tool.id)
        .await
        .tool_results()
        .map(|(_, state)| state.clone())
        .collect();
    assert_eq!(
        states,
        vec![agent::ToolResultState::Unknown],
        "在途工具调用记为结果未知"
    );
}
