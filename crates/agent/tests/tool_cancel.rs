use std::time::Duration;

use reqwest::StatusCode;

mod support;

use support::provider::ToolCallSpec;
use support::{Harness, Script};

/// 规格 A-10 与 A-37：工具批次中途取消时，剩余调用以「结果未知」入树，保持调用与结果配对。
#[tokio::test]
async fn abandons_remaining_tool_calls_when_cancelled() {
    let harness = Harness::start_with(
        agent::EngineConfig::default(),
        vec![
            Script::ToolCalls(vec![
                ToolCallSpec::new(
                    "call_slow",
                    "bash",
                    r#"{"command":"touch started.txt; sleep 5"}"#,
                ),
                ToolCallSpec::new("call_next", "bash", r#"{"command":"echo next"}"#),
            ]),
            Script::Text("继续".into()),
        ],
    )
    .await;
    let session = harness.session("workspace").await;
    let marker = harness.workspace_path("workspace").join("started.txt");

    harness.submit_message(&session.id, "两个工具").await;
    for _ in 0..250 {
        if marker.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(marker.exists(), "第一个工具已经开始执行");

    let response = harness.cancel_run(&session.id).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let tree = harness.wait_idle(&session.id).await;
    assert_eq!(tree.runs[0].status, agent::RunStatus::Cancelled);

    let tools: Vec<&agent::MessageSkeleton> = tree
        .nodes
        .iter()
        .filter(|node| node.role == agent::MessageRole::Tool)
        .collect();
    assert_eq!(tools.len(), 2, "两条调用都有对应的 tool 节点");
    let mut states = Vec::new();
    for node in tools {
        states.extend(
            harness
                .body(&session.id, node.id)
                .await
                .tool_results()
                .map(|(call_id, state)| (call_id.to_owned(), state.clone())),
        );
    }
    let next = states
        .iter()
        .find(|(call_id, _)| call_id == "call_next")
        .expect("剩余调用的节点");
    assert_eq!(
        next.1,
        agent::ToolResultState::Unknown,
        "未执行的调用记为结果未知"
    );

    harness.submit_message(&session.id, "继续").await;
    let tree = harness.wait_idle(&session.id).await;
    assert_eq!(tree.runs[1].status, agent::RunStatus::Succeeded);

    let request = harness
        .provider
        .requests()
        .last()
        .cloned()
        .expect("续跑的请求");
    let messages = request["messages"].as_array().expect("请求带消息数组");
    let mut tool_call_ids: Vec<String> = messages
        .iter()
        .filter(|message| message["role"] == "tool")
        .map(|message| {
            message["tool_call_id"]
                .as_str()
                .expect("工具消息带 tool_call_id")
                .to_owned()
        })
        .collect();
    tool_call_ids.sort();
    assert_eq!(
        tool_call_ids,
        vec!["call_next".to_owned(), "call_slow".to_owned()],
        "续跑请求里每条调用都有对应的工具结果"
    );
}
