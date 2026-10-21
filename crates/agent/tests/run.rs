use std::time::Duration;

use reqwest::StatusCode;

mod support;

use support::provider::ToolCallSpec;
use support::{Harness, Script};

fn user_texts(tree: &agent::SessionTree) -> Vec<&str> {
    tree.nodes
        .iter()
        .filter(|node| node.role == agent::MessageRole::User)
        .map(|node| node.text.as_str())
        .collect()
}

/// 规格 user story A-8：assistant 文本流式落库，run 结束后内容一致。
#[tokio::test]
async fn streams_assistant_text_and_finishes_run() {
    let harness = Harness::start_with(
        agent::EngineConfig::default(),
        vec![Script::Text("你好，世界".into())],
    )
    .await;
    let session = harness.session("workspace").await;

    let response = harness.submit_message(&session.id, "打个招呼").await;
    assert_eq!(response.status(), StatusCode::CREATED);

    let tree = harness.wait_idle(&session.id).await;
    assert!(!tree.session.busy);
    assert!(tree.queue.is_empty());
    assert_eq!(tree.nodes.len(), 2);
    let user = &tree.nodes[0];
    let assistant = &tree.nodes[1];
    assert_eq!(user.role, agent::MessageRole::User);
    assert_eq!(user.text.as_str(), "打个招呼");
    assert_eq!(user.parent_id, None);
    assert_eq!(assistant.role, agent::MessageRole::Assistant);
    assert_eq!(assistant.text.as_str(), "你好，世界");
    assert_eq!(assistant.parent_id, Some(user.id));
    assert_eq!(tree.active_leaf, Some(assistant.id));

    assert_eq!(tree.runs.len(), 1);
    let run = &tree.runs[0];
    assert_eq!(run.status, agent::RunStatus::Succeeded);
    assert_eq!(run.failure, None);
    assert_eq!(user.run_id, Some(run.id));
    assert_eq!(assistant.run_id, Some(run.id));
}

/// 规格「事实事件」：run 开始与结束的事实事件顺序稳定。
#[tokio::test]
async fn records_run_lifecycle_events_in_order() {
    let harness = Harness::start_with(
        agent::EngineConfig::default(),
        vec![Script::Text("完成".into())],
    )
    .await;
    let session = harness.session("workspace").await;
    harness.submit_message(&session.id, "执行").await;
    harness.wait_idle(&session.id).await;

    assert_eq!(
        support::recorded_event_kinds(&harness.database).await,
        vec![
            "workspace_registered",
            "session_created",
            "message_queued",
            "message_appended",
            "message_appended",
            "active_leaf_changed",
            "queue_item_removed",
            "run_started",
            "message_updated",
            "run_finished",
        ]
    );
}

/// 规格「模型调用」：同一轮多个工具调用按序执行，收集结果后再请求下一轮。
#[tokio::test]
async fn executes_tool_calls_in_order_and_continues() {
    let harness = Harness::start_with(
        agent::EngineConfig::default(),
        vec![
            Script::ToolCalls(vec![
                ToolCallSpec::new(
                    "call_1",
                    "write_file",
                    r#"{"path":"notes.txt","content":"记录"}"#,
                ),
                ToolCallSpec::new("call_2", "read_file", r#"{"path":"notes.txt"}"#),
            ]),
            Script::Text("完成".into()),
        ],
    )
    .await;
    let session = harness.session("workspace").await;
    harness.submit_message(&session.id, "写入并读取").await;
    let tree = harness.wait_idle(&session.id).await;

    let assistant = &tree.nodes[1];
    assert_eq!(assistant.tool_calls.len(), 2);
    assert_eq!(assistant.tool_calls[0].name, "write_file");
    assert_eq!(
        assistant.tool_calls[0].outcome,
        agent::ToolOutcome::Succeeded {
            content: "写入 6 字节: notes.txt".into(),
        }
    );
    assert_eq!(assistant.tool_calls[1].name, "read_file");
    assert_eq!(
        assistant.tool_calls[1].outcome,
        agent::ToolOutcome::Succeeded {
            content: "1\t记录\n".into(),
        }
    );
    assert_eq!(assistant.text.as_str(), "完成");
    assert_eq!(tree.runs[0].status, agent::RunStatus::Succeeded);
    assert_eq!(
        std::fs::read_to_string(harness.workspace_path("workspace").join("notes.txt"))
            .expect("工具写入文件"),
        "记录"
    );

    assert_eq!(
        support::recorded_event_kinds(&harness.database).await,
        vec![
            "workspace_registered",
            "session_created",
            "message_queued",
            "message_appended",
            "message_appended",
            "active_leaf_changed",
            "queue_item_removed",
            "run_started",
            "tool_call_recorded",
            "tool_call_recorded",
            "message_updated",
            "run_finished",
        ]
    );

    let requests = harness.provider.requests();
    assert_eq!(requests.len(), 2, "每个生成步一次模型请求");
    let tools = requests[0]["tools"].as_array().expect("请求携带工具定义");
    let names: Vec<&str> = tools
        .iter()
        .map(|tool| tool["function"]["name"].as_str().expect("工具定义有名称"))
        .collect();
    assert_eq!(names, vec!["read_file", "write_file", "edit_file", "bash"]);

    let messages = requests[1]["messages"]
        .as_array()
        .expect("第二轮请求含 messages");
    let results: Vec<&serde_json::Value> = messages
        .iter()
        .filter(|message| message["role"] == "tool")
        .collect();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["tool_call_id"], "call_1");
    assert_eq!(results[0]["content"], "写入 6 字节: notes.txt");
    assert_eq!(results[1]["tool_call_id"], "call_2");
    assert_eq!(results[1]["content"], "1\t记录\n");
}

/// 规格 user story A-15 与「调度与会话状态」：同一 session 的队列按 FIFO 执行。
#[tokio::test]
async fn executes_queued_messages_in_fifo_order() {
    let harness = Harness::start_with(
        agent::EngineConfig::default(),
        vec![
            Script::Text("A".into()),
            Script::Text("B".into()),
            Script::Text("C".into()),
        ],
    )
    .await;
    let session = harness.session("workspace").await;

    for text in ["一", "二", "三"] {
        assert_eq!(
            harness.submit_message(&session.id, text).await.status(),
            StatusCode::CREATED
        );
    }
    let tree = harness.wait_idle(&session.id).await;

    assert_eq!(user_texts(&tree), vec!["一", "二", "三"]);
    let assistant_texts: Vec<&str> = tree
        .nodes
        .iter()
        .filter(|node| node.role == agent::MessageRole::Assistant)
        .map(|node| node.text.as_str())
        .collect();
    assert_eq!(assistant_texts, vec!["A", "B", "C"]);
    assert_eq!(tree.runs.len(), 3);
    assert!(
        tree.runs
            .iter()
            .all(|run| run.status == agent::RunStatus::Succeeded)
    );
}

/// 规格 user story A-11：删除尚未开始的队列项后该项不再执行。
#[tokio::test]
async fn removes_pending_queue_item_before_execution() {
    let harness = Harness::start_with(
        agent::EngineConfig::default(),
        vec![Script::Hang, Script::Text("B".into())],
    )
    .await;
    let session = harness.session("workspace").await;

    harness.submit_message(&session.id, "一").await;
    let second: agent::QueueItem = harness
        .submit_message(&session.id, "二")
        .await
        .json()
        .await
        .expect("解析队列项");
    let third: agent::QueueItem = harness
        .submit_message(&session.id, "三")
        .await
        .json()
        .await
        .expect("解析队列项");

    let running = harness.wait_busy(&session.id).await;
    assert_eq!(
        running.queue.iter().map(|item| item.id).collect::<Vec<_>>(),
        vec![second.id, third.id]
    );

    let response = harness.remove_queue_item(&session.id, third.id).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let response = harness.cancel_run(&session.id).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let tree = harness.wait_idle(&session.id).await;
    assert_eq!(user_texts(&tree), vec!["一", "二"]);
    assert_eq!(tree.queue.len(), 0);
    assert_eq!(tree.runs[0].status, agent::RunStatus::Cancelled);
    assert_eq!(tree.runs[1].status, agent::RunStatus::Succeeded);
}

/// 规格 user story A-10：取消保留已收到的部分 assistant 文本，队列继续。
#[tokio::test]
async fn cancels_running_run_and_keeps_partial_text() {
    let harness = Harness::start_with(
        agent::EngineConfig::default(),
        vec![
            Script::PartialThenHang("部分输出".into()),
            Script::Text("下一项".into()),
        ],
    )
    .await;
    let session = harness.session("workspace").await;

    harness.submit_message(&session.id, "开始").await;
    harness.submit_message(&session.id, "队列中的消息").await;
    harness
        .wait_for_tree(&session.id, |tree| {
            tree.nodes.iter().any(|node| {
                node.role == agent::MessageRole::Assistant && !node.text.as_str().is_empty()
            })
        })
        .await;

    let response = harness.cancel_run(&session.id).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let tree = harness.wait_idle(&session.id).await;
    assert_eq!(tree.runs[0].status, agent::RunStatus::Cancelled);
    assert_eq!(tree.runs[0].failure, None);
    assert_eq!(tree.nodes[1].text.as_str(), "部分输出");
    assert_eq!(tree.runs[1].status, agent::RunStatus::Succeeded);
    assert_eq!(tree.nodes[3].text.as_str(), "下一项");
}

/// 规格 user story A-23：多个标签页订阅同一条事件流，任一命令对全部订阅者生效。
#[tokio::test]
async fn broadcasts_facts_to_every_subscriber() {
    let harness = Harness::start_with(
        agent::EngineConfig::default(),
        vec![Script::Text("完成".into())],
    )
    .await;
    let session = harness.session("workspace").await;
    let cursor = harness.bootstrap().await.max_event_id;
    let mut first = harness.subscribe(cursor).await;
    let mut second = harness.subscribe(cursor).await;

    harness.submit_message(&session.id, "执行").await;

    for events in [&mut first, &mut second] {
        let mut kinds = Vec::new();
        loop {
            let event = tokio::time::timeout(Duration::from_secs(10), events.next())
                .await
                .expect("等待 SSE 事件")
                .expect("事件流未结束");
            let agent::StreamEvent::Fact { event, .. } = event else {
                continue;
            };
            let kind = serde_json::to_value(&event).expect("事件可序列化")["kind"]
                .as_str()
                .expect("事件有 kind")
                .to_owned();
            kinds.push(kind);
            if matches!(event, agent::FactEvent::RunFinished { .. }) {
                break;
            }
        }
        assert_eq!(
            kinds,
            vec![
                "message_queued",
                "message_appended",
                "message_appended",
                "active_leaf_changed",
                "queue_item_removed",
                "run_started",
                "message_updated",
                "run_finished",
            ]
        );
    }
}

/// 规格 user story A-12：工具失败以结构化结果回传模型，run 继续。
#[tokio::test]
async fn passes_tool_failure_to_model_and_continues() {
    let harness = Harness::start_with(
        agent::EngineConfig::default(),
        vec![
            Script::ToolCall {
                id: "call_1".into(),
                name: "read_file".into(),
                arguments: r#"{"path":"absent.txt"}"#.into(),
            },
            Script::Text("已记录失败".into()),
        ],
    )
    .await;
    let session = harness.session("workspace").await;
    harness
        .submit_message(&session.id, "读取不存在的文件")
        .await;
    let tree = harness.wait_idle(&session.id).await;

    let assistant = &tree.nodes[1];
    assert_eq!(
        assistant.tool_calls[0].outcome,
        agent::ToolOutcome::Failed {
            code: agent::ToolErrorCode::FileNotFound,
            message: "文件不存在: absent.txt".into(),
        }
    );
    assert_eq!(tree.runs[0].status, agent::RunStatus::Succeeded);
    assert_eq!(assistant.text.as_str(), "已记录失败");

    let requests = harness.provider.requests();
    assert_eq!(requests.len(), 2, "失败后继续请求下一轮");
    let messages = requests[1]["messages"]
        .as_array()
        .expect("第二轮请求含 messages");
    let result = messages
        .iter()
        .find(|message| message["role"] == "tool")
        .expect("工具结果以 tool 消息回传");
    assert_eq!(result["tool_call_id"], "call_1");
    assert_eq!(
        result["content"],
        r#"{"error":"file_not_found","message":"文件不存在: absent.txt"}"#
    );
}

/// 规格「事实事件」与 user story 9：工具调用在 run 结束前就以事实事件广播。
#[tokio::test]
async fn broadcasts_tool_call_fact_while_run_is_running() {
    let harness = Harness::start_with(
        agent::EngineConfig::default(),
        vec![
            Script::ToolCall {
                id: "call_1".into(),
                name: "write_file".into(),
                arguments: r#"{"path":"notes.txt","content":"记录"}"#.into(),
            },
            Script::Hang,
        ],
    )
    .await;
    let session = harness.session("workspace").await;
    let cursor = harness.bootstrap().await.max_event_id;
    let mut events = harness.subscribe(cursor).await;

    harness.submit_message(&session.id, "写入").await;

    let mut kinds = Vec::new();
    let recorded = loop {
        let event = tokio::time::timeout(Duration::from_secs(10), events.next())
            .await
            .expect("等待 SSE 事件")
            .expect("事件流未结束");
        let agent::StreamEvent::Fact { event, .. } = event else {
            continue;
        };
        let kind = serde_json::to_value(&event).expect("事件可序列化")["kind"]
            .as_str()
            .expect("事件有 kind")
            .to_owned();
        kinds.push(kind);
        if let agent::FactEvent::ToolCallRecorded { tool_call, .. } = event {
            break tool_call;
        }
        assert!(
            !matches!(event, agent::FactEvent::RunFinished { .. }),
            "工具调用必须在 run 结束前广播: {kinds:?}"
        );
    };

    assert_eq!(recorded.name, "write_file");
    assert_eq!(
        recorded.outcome,
        agent::ToolOutcome::Succeeded {
            content: "写入 6 字节: notes.txt".into(),
        }
    );

    let tree = harness.tree(&session.id).await;
    assert!(tree.session.busy, "run 仍在运行");
    assert_eq!(tree.nodes[1].tool_calls, vec![recorded]);

    let response = harness.cancel_run(&session.id).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    harness.wait_idle(&session.id).await;
}
