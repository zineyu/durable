#![expect(
    clippy::panic,
    reason = "非 `#[test]` 函数中的轮询等待超时后用 panic 直接报告预期状态未到达。"
)]

use reqwest::StatusCode;

mod support;

use support::{Harness, Script, connect, error_code, recorded_facts};

fn queue_texts(tree: &agent::SessionTree) -> Vec<&str> {
    tree.queue.iter().map(|item| item.text.as_str()).collect()
}

/// 按角色取整棵树的文本正文，顺序为消息 id 升序。
async fn role_texts(
    harness: &Harness,
    session: &agent::SessionId,
    tree: &agent::SessionTree,
    role: agent::MessageRole,
) -> Vec<String> {
    let messages: Vec<agent::MessageId> = tree
        .nodes
        .iter()
        .filter(|node| node.role == role)
        .map(|node| node.id)
        .collect();
    harness
        .bodies(session, &messages)
        .await
        .into_iter()
        .map(|body| body.text())
        .collect()
}

async fn busy_ids(harness: &Harness, sessions: &[agent::SessionMetadata]) -> Vec<agent::SessionId> {
    let mut busy = Vec::new();
    for session in sessions {
        if harness.tree(&session.id).await.session.busy {
            busy.push(session.id.clone());
        }
    }
    busy
}

async fn wait_for_busy_count(
    harness: &Harness,
    sessions: &[agent::SessionMetadata],
    count: usize,
) -> Vec<agent::SessionId> {
    for _ in 0..500 {
        let busy = busy_ids(harness, sessions).await;
        if busy.len() == count {
            return busy;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("同时运行的 run 未达到 {count} 个")
}

/// 规格「调度与会话状态」：run 进行中提交的消息留在队列，不打断当前 run。
#[tokio::test]
async fn queues_messages_behind_running_run() {
    let harness = Harness::start_with(agent::EngineConfig::default(), vec![Script::Hang]).await;
    let session = harness.session("workspace").await;

    let response = harness.submit_message(&session.id, "第一条").await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let second = harness
        .submit_message(&session.id, "第二条")
        .await
        .json::<agent::QueueItem>()
        .await
        .expect("解析队列项");
    let third = harness
        .submit_message(&session.id, "第三条")
        .await
        .json::<agent::QueueItem>()
        .await
        .expect("解析队列项");

    let running = harness.wait_busy(&session.id).await;
    harness.wait_for_requests(1).await;
    assert_eq!(running.queue, vec![second.clone(), third.clone()]);
    assert_eq!(running.session.queue_length, 2);
    assert_eq!(queue_texts(&running), vec!["第二条", "第三条"]);

    let bootstrap = harness.bootstrap().await;
    assert_eq!(bootstrap.sessions[0].queue_length, 2);
    assert!(bootstrap.sessions[0].busy);

    harness.cancel_run(&session.id).await;
    harness.wait_idle(&session.id).await;
}

/// 规格 user story A-15：同一 session 的消息按 FIFO 顺序执行。
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

    for text in ["第一条", "第二条", "第三条"] {
        assert_eq!(
            harness.submit_message(&session.id, text).await.status(),
            StatusCode::CREATED
        );
    }

    let tree = harness.wait_idle(&session.id).await;
    let users = role_texts(&harness, &session.id, &tree, agent::MessageRole::User).await;
    assert_eq!(users, vec!["第一条", "第二条", "第三条"]);
    assert!(tree.queue.is_empty());
    assert_eq!(tree.session.queue_length, 0);
}

/// ADR 0037「崩溃恢复」：重启后接管仍是 running 的 run 从树尾续跑，队列继续执行。
#[tokio::test]
async fn continues_queue_after_restart() {
    let harness = Harness::start_with(agent::EngineConfig::default(), vec![Script::Hang]).await;
    let session = harness.session("workspace").await;

    harness.submit_message(&session.id, "重启前 A").await;
    harness.submit_message(&session.id, "重启前 B").await;
    harness.wait_busy(&session.id).await;
    harness.wait_for_requests(1).await;

    harness.provider.push(Script::Text("A 完成".into()));
    harness.provider.push(Script::Text("B 完成".into()));
    let harness = harness.restart().await;
    let tree = harness.wait_idle(&session.id).await;

    assert_eq!(tree.runs.len(), 2, "崩溃不产生新的 run");
    assert_eq!(tree.runs[0].status, agent::RunStatus::Succeeded);
    assert_eq!(tree.runs[1].status, agent::RunStatus::Succeeded);
    let users = role_texts(&harness, &session.id, &tree, agent::MessageRole::User).await;
    assert_eq!(users, vec!["重启前 A", "重启前 B"]);
    let assistants = role_texts(&harness, &session.id, &tree, agent::MessageRole::Assistant).await;
    assert_eq!(
        assistants,
        vec!["A 完成", "B 完成"],
        "中断的步重跑，队列项接着执行"
    );
}

/// 规格 user story A-4：首条用户消息决定标题，后续消息不再改变。
#[tokio::test]
async fn derives_auto_title_from_first_message() {
    let harness = Harness::start().await;
    let session = harness.session("workspace").await;
    assert_eq!(session.title.as_str(), agent::DEFAULT_SESSION_TITLE);

    harness.submit_message(&session.id, "第一行\n第二行").await;
    let titled = harness.tree(&session.id).await;
    assert_eq!(titled.session.title.as_str(), "第一行");

    harness.submit_message(&session.id, "另一条消息").await;
    assert_eq!(
        harness.tree(&session.id).await.session.title.as_str(),
        "第一行",
        "第二条消息不改变自动标题"
    );
}

/// 规格 user story A-4：标题取首行前 50 个字符。
#[tokio::test]
async fn truncates_auto_title_to_fifty_characters() {
    let harness = Harness::start().await;
    let session = harness.session("workspace").await;
    let long = format!("{}\n第二行", "字".repeat(60));

    harness.submit_message(&session.id, &long).await;

    assert_eq!(
        harness.tree(&session.id).await.session.title.as_str(),
        "字".repeat(50)
    );
}

/// 规格 user story A-4：重命名后不再自动更新标题。
#[tokio::test]
async fn keeps_renamed_title_before_first_message() {
    let harness = Harness::start().await;
    let session = harness.session("workspace").await;

    let response = harness.rename_session(&session.id, "自定义标题").await;
    assert_eq!(response.status(), StatusCode::OK);

    harness.submit_message(&session.id, "首条消息").await;

    let tree = harness.tree(&session.id).await;
    assert_eq!(tree.session.title.as_str(), "自定义标题");
}

/// 规格「边界解析」：空白正文不入队，也不改变标题与队列。
#[tokio::test]
async fn rejects_blank_message_text() {
    let harness = Harness::start().await;
    let session = harness.session("workspace").await;

    for text in ["", "   ", "\n\t "] {
        let response = harness.submit_message(&session.id, text).await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(error_code(response).await, agent::ErrorCode::InvalidMessage);
    }

    let tree = harness.tree(&session.id).await;
    assert!(tree.queue.is_empty());
    assert_eq!(tree.session.queue_length, 0);
    assert_eq!(tree.session.title.as_str(), agent::DEFAULT_SESSION_TITLE);
    assert!(tree.nodes.is_empty());
}

/// 消息只能提交到已存在的 session。
#[tokio::test]
async fn rejects_message_for_missing_session() {
    let harness = Harness::start().await;
    let missing = agent::SessionId::parse("missing").expect("合法 session id");

    let response = harness.submit_message(&missing, "你好").await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        error_code(response).await,
        agent::ErrorCode::SessionNotFound
    );
}

/// 规格 user story A-5：删除运行中的 session 先取消 run，并丢弃队列。
#[tokio::test]
async fn deletes_running_session_with_its_queue() {
    let harness = Harness::start_with(agent::EngineConfig::default(), vec![Script::Hang]).await;
    let session = harness.session("workspace").await;
    harness.submit_message(&session.id, "运行中").await;
    harness.submit_message(&session.id, "排队中").await;
    harness.wait_busy(&session.id).await;
    harness.wait_for_requests(1).await;

    let response = harness.delete_session(&session.id).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let response = harness.session_tree(&session.id).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM queue_item")
        .fetch_one(&mut connect(&harness.database).await)
        .await
        .expect("统计队列项");
    assert_eq!(remaining, 0, "session 删除后不保留队列项");
}

/// 规格「调度与会话状态」：session 内的 FIFO 串行，session 之间的 run 同时启动，没有全局并发上限。
#[tokio::test]
async fn starts_runs_across_sessions_simultaneously() {
    let harness = Harness::start_with(agent::EngineConfig::default(), vec![Script::Hang; 3]).await;
    let (_, workspace) = harness.registered_workspace("workspace").await;
    let sessions = vec![
        harness.created_session(&workspace.path).await,
        harness.created_session(&workspace.path).await,
        harness.created_session(&workspace.path).await,
    ];
    for session in &sessions {
        harness.submit_message(&session.id, "运行").await;
    }

    let running = wait_for_busy_count(&harness, &sessions, 3).await;
    assert_eq!(running.len(), 3, "三个 session 的 run 必须同时运行");
    for session in &sessions {
        let tree = harness.tree(&session.id).await;
        assert!(tree.queue.is_empty(), "启动后队列项离开队列");
    }
}

/// 规格「事实事件」：入队写入 `message_queued`，携带队列项与更新后的 session 元数据。
#[tokio::test]
async fn records_message_queued_event() {
    let harness = Harness::start_with(agent::EngineConfig::default(), vec![Script::Hang]).await;
    let session = harness.session("workspace").await;
    let item: agent::QueueItem = harness
        .submit_message(&session.id, "记录事件")
        .await
        .json()
        .await
        .expect("解析队列项");

    let fact = recorded_facts(&harness.database)
        .await
        .into_iter()
        .find(|fact| matches!(fact.event, agent::FactEvent::MessageQueued { .. }))
        .expect("记录了入队事件");
    let agent::FactEvent::MessageQueued { item: queued, .. } = fact.event else {
        panic!("事件是 message_queued");
    };
    let recorded = fact.session.expect("入队事件带 session 视图");
    assert_eq!(queued, item);
    assert_eq!(recorded.id, session.id);
    assert_eq!(recorded.title.as_str(), "记录事件");
    assert_eq!(recorded.updated_at, item.created_at);

    harness.wait_busy(&session.id).await;
    harness.wait_for_requests(1).await;
    harness.cancel_run(&session.id).await;
    harness.wait_idle(&session.id).await;
}

/// 队列项 id 与消息 id 共享全局分配语义：跨 session 单调递增。
#[tokio::test]
async fn allocates_queue_item_ids_across_sessions() {
    let harness = Harness::start().await;
    let first_session = harness.session("first").await;
    let second_session = harness.session("second").await;

    let first: agent::QueueItem = harness
        .submit_message(&first_session.id, "第一个 session")
        .await
        .json()
        .await
        .expect("解析队列项");
    let second: agent::QueueItem = harness
        .submit_message(&second_session.id, "第二个 session")
        .await
        .json()
        .await
        .expect("解析队列项");

    assert!(second.id > first.id, "队列项 id 全局递增");
}

/// ADR 0012「单一写入线」：并发入队被串行化，执行顺序与分配 id 顺序一致。
#[tokio::test]
async fn serializes_concurrent_submissions_into_fifo_order() {
    let harness = Harness::start_with(
        agent::EngineConfig::default(),
        vec![
            Script::Text("A".into()),
            Script::Text("B".into()),
            Script::Text("C".into()),
            Script::Text("D".into()),
        ],
    )
    .await;
    let session = harness.session("workspace").await;

    let responses = tokio::join!(
        harness.submit_message(&session.id, "a"),
        harness.submit_message(&session.id, "b"),
        harness.submit_message(&session.id, "c"),
        harness.submit_message(&session.id, "d"),
    );
    let mut items = Vec::new();
    for response in [responses.0, responses.1, responses.2, responses.3] {
        assert_eq!(response.status(), StatusCode::CREATED);
        items.push(
            response
                .json::<agent::QueueItem>()
                .await
                .expect("解析队列项"),
        );
    }
    items.sort_by_key(|item| item.id);

    let mut ids: Vec<i64> = items.iter().map(|item| item.id.get()).collect();
    ids.dedup();
    assert_eq!(ids.len(), 4, "每次入队分配独立的队列项 id");

    let expected: Vec<String> = items
        .iter()
        .map(|item| item.text.as_str().to_owned())
        .collect();
    let tree = harness.wait_idle(&session.id).await;
    let users = role_texts(&harness, &session.id, &tree, agent::MessageRole::User).await;
    assert_eq!(users, expected, "执行顺序等于队列项 id 顺序");
}

/// 分配器独立于队列行：删除 session 不回收已分配的队列项 id。
#[tokio::test]
async fn keeps_queue_item_id_allocator_after_session_deletion() {
    let harness = Harness::start().await;
    let first = harness.session("first").await;
    let before: agent::QueueItem = harness
        .submit_message(&first.id, "删除前")
        .await
        .json()
        .await
        .expect("解析队列项");

    let response = harness.delete_session(&first.id).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let second = harness.session("second").await;
    let after: agent::QueueItem = harness
        .submit_message(&second.id, "删除后")
        .await
        .json()
        .await
        .expect("解析队列项");
    assert!(
        after.id > before.id,
        "队列项 id 不复用已删除的编号: {} <= {}",
        after.id.get(),
        before.id.get()
    );
}
