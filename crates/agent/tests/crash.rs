//! 进程级崩溃恢复：流式中途被杀与工具执行中被杀，重启后的消息树与续跑请求。
//! 规格 user story A-38 与 ADR 0047：流式进度落库。内核层的对应分支在 `crates/core/tests/progress.rs`。

#![expect(
    clippy::panic,
    reason = "进程级测试用 panic 直接报告夹具失败；其余断言用 assert 宏。"
)]

use std::path::PathBuf;
use std::time::Duration;

mod support;

use support::process::{Process, create_session, nodes_of, submit, text_of, wait_for, wait_idle};
use support::provider::ToolCallSpec;
use support::{Script, ScriptedProvider};

/// 等待流式进度落库。进度是进程被杀之后唯一留下的部分输出。
async fn wait_for_progress(
    process: &Process,
    session: &agent::SessionId,
    expected: &str,
) -> agent::SessionTreeSnapshot {
    wait_for(process, session, |tree| {
        tree.runs
            .iter()
            .any(|run| run.progress.as_deref() == Some(expected))
    })
    .await
}

/// 等待工作副本中的标记文件出现，即工具已经开始执行。
async fn wait_for_marker(marker: &std::path::Path) {
    for _ in 0..250 {
        if marker.exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("标记文件未在预期时间内出现: {}", marker.display());
}

/// 规格 user story A-38：流式中途杀掉进程，重启后部分输出入树为已中断消息，续跑接着它继续。
#[tokio::test]
async fn kills_mid_stream_and_keeps_partial_text() {
    let provider = ScriptedProvider::start(vec![
        Script::PartialThenHang("部分输出".into()),
        Script::Text("续写完成。".into()),
    ])
    .await;
    let directory = tempfile::tempdir().expect("创建临时目录");
    let database = directory.path().join("agent.sqlite");
    let workspace = directory.path().join("workspace");
    std::fs::create_dir(&workspace).expect("创建 workspace 目录");

    let process = Process::start(&database, &workspace, &provider).await;
    let session = create_session(&process, &workspace).await;
    submit(&process, &session.id, "写一段长回答").await;
    wait_for_progress(&process, &session.id, "部分输出").await;
    process.kill().await;

    let process = Process::start(&database, &workspace, &provider).await;
    let tree = wait_idle(&process, &session.id).await;

    let assistants = nodes_of(&tree, agent::MessageRole::Assistant);
    assert_eq!(assistants.len(), 2, "部分输出与续跑各一条 assistant 消息");
    assert!(assistants[0].interrupted, "部分输出标注已中断");
    assert!(!assistants[1].interrupted, "续跑产出的是普通回答");
    assert_eq!(
        text_of(&process, &session.id, assistants[0].id).await,
        "部分输出"
    );
    assert_eq!(
        text_of(&process, &session.id, assistants[1].id).await,
        "续写完成。"
    );
    assert_eq!(tree.runs[0].status, agent::RunStatus::Succeeded);
    assert_eq!(tree.runs[0].progress, None, "run 结束时清空进度");

    let requests = provider.requests();
    let resumed = requests.last().cloned().expect("续跑发出的模型请求");
    let messages = resumed["messages"].as_array().expect("请求带消息数组");
    assert!(
        messages
            .iter()
            .any(|message| message["role"] == "assistant" && message["content"] == "部分输出"),
        "模型看到自己已经写过的部分输出: {resumed}"
    );

    process.kill().await;
}

/// 规格 user story A-24 与 ADR 0037：工具执行中被杀，重启后非幂等工具的结局为「结果未知」。
#[tokio::test]
async fn kills_during_a_tool_and_reports_an_unknown_outcome() {
    let provider = ScriptedProvider::start(vec![
        Script::ToolCalls(vec![ToolCallSpec::new(
            "call_slow",
            "bash",
            r#"{"command":"touch started.txt; sleep 5"}"#,
        )]),
        Script::Text("工具之后的回答".into()),
    ])
    .await;
    let directory = tempfile::tempdir().expect("创建临时目录");
    let database = directory.path().join("agent.sqlite");
    let workspace = directory.path().join("workspace");
    std::fs::create_dir(&workspace).expect("创建 workspace 目录");

    let process = Process::start(&database, &workspace, &provider).await;
    let session = create_session(&process, &workspace).await;
    submit(&process, &session.id, "执行一个慢命令").await;
    let marker: PathBuf = workspace.join("started.txt");
    wait_for_marker(&marker).await;
    process.kill().await;

    let process = Process::start(&database, &workspace, &provider).await;
    let tree = wait_idle(&process, &session.id).await;

    let tools = nodes_of(&tree, agent::MessageRole::Tool);
    assert_eq!(tools.len(), 1, "被切断的调用保留它自己的节点");
    let response = reqwest::Client::new()
        .post(process.url(&format!(
            "/api/sessions/{}/message-bodies",
            session.id.as_str()
        )))
        .json(&serde_json::json!({ "messageIds": [tools[0].id.get()] }))
        .send()
        .await
        .expect("读取工具正文");
    let body: agent::MessageBodiesResponse = response.json().await.expect("解析工具正文");
    let states: Vec<agent::ToolResultState> = body
        .messages
        .first()
        .map(|message| {
            message
                .tool_results()
                .map(|(_, state)| state.clone())
                .collect()
        })
        .expect("工具节点有正文");
    assert_eq!(
        states,
        vec![agent::ToolResultState::Unknown],
        "非幂等工具的结局记为结果未知"
    );

    let assistants = nodes_of(&tree, agent::MessageRole::Assistant);
    assert_eq!(assistants.len(), 2, "工具调用的消息与续跑的回答各一条");
    assert!(
        assistants.iter().all(|node| !node.interrupted),
        "工具执行中被杀不产生已中断消息"
    );
    assert_eq!(
        text_of(&process, &session.id, assistants[1].id).await,
        "工具之后的回答"
    );

    let requests = provider.requests();
    let resumed = requests.last().cloned().expect("续跑发出的模型请求");
    let messages = resumed["messages"].as_array().expect("请求带消息数组");
    assert!(
        messages.iter().any(|message| {
            message["role"] == "tool"
                && message["content"]
                    .as_str()
                    .is_some_and(|content| content.contains("结果未知"))
        }),
        "续跑请求里带上工具的未知结局: {resumed}"
    );

    process.kill().await;
}
