use reqwest::StatusCode;

mod support;

use support::{Harness, connect, error_code, recorded_event_kinds};

async fn tree(harness: &Harness, id: &agent::SessionId) -> agent::SessionTree {
    harness
        .session_tree(id)
        .await
        .json()
        .await
        .expect("解析消息树")
}

fn queue_texts(tree: &agent::SessionTree) -> Vec<&str> {
    tree.queue.iter().map(|item| item.text.as_str()).collect()
}

/// 规格「调度与会话状态」：入队只写队列，不创建消息节点，不移动 active leaf。
#[tokio::test]
async fn enqueues_message_without_creating_tree_node() {
    let harness = Harness::start().await;
    let session = harness.session("workspace").await;

    let response = harness.submit_message(&session.id, "检查仓库状态").await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let item: agent::QueueItem = response.json().await.expect("解析队列项");
    assert!(item.id.get() > 0);
    assert_eq!(item.text.as_str(), "检查仓库状态");
    assert!(item.created_at > 0);

    let tree = tree(&harness, &session.id).await;
    assert_eq!(tree.active_leaf, None);
    assert!(tree.nodes.is_empty(), "入队不创建消息节点");
    assert_eq!(tree.queue, vec![item.clone()]);
    assert_eq!(tree.session.queue_length, 1);
    assert!(!tree.session.busy, "run 切片前 session 不进入忙碌状态");
    assert_eq!(tree.session.updated_at, item.created_at);
}

/// 规格 user story 7：session 内队列按 FIFO 执行，id 升序即入队顺序。
#[tokio::test]
async fn keeps_queue_in_fifo_order() {
    let harness = Harness::start().await;
    let session = harness.session("workspace").await;

    let mut items = Vec::new();
    for text in ["第一条", "第二条", "第三条"] {
        let response = harness.submit_message(&session.id, text).await;
        assert_eq!(response.status(), StatusCode::CREATED);
        items.push(
            response
                .json::<agent::QueueItem>()
                .await
                .expect("解析队列项"),
        );
    }

    assert!(items[0].id < items[1].id && items[1].id < items[2].id);
    let tree = tree(&harness, &session.id).await;
    assert_eq!(tree.queue, items);
    assert_eq!(queue_texts(&tree), vec!["第一条", "第二条", "第三条"]);
    assert_eq!(tree.session.queue_length, 3);

    let bootstrap = harness.bootstrap().await;
    assert_eq!(bootstrap.sessions[0].queue_length, 3);
}

/// 规格「持久化」：重启后队列、顺序与队列长度保持，分配器继续递增。
#[tokio::test]
async fn restores_queue_after_restart() {
    let harness = Harness::start().await;
    let session = harness.session("workspace").await;
    let mut items = Vec::new();
    for text in ["重启前 A", "重启前 B"] {
        let response = harness.submit_message(&session.id, text).await;
        items.push(
            response
                .json::<agent::QueueItem>()
                .await
                .expect("解析队列项"),
        );
    }

    let harness = harness.restart().await;
    let restored = tree(&harness, &session.id).await;
    assert_eq!(restored.queue, items);
    assert_eq!(restored.session.queue_length, 2);
    assert!(restored.nodes.is_empty());
    assert_eq!(restored.active_leaf, None);

    let next: agent::QueueItem = harness
        .submit_message(&session.id, "重启后的消息")
        .await
        .json()
        .await
        .expect("解析新队列项");
    assert!(next.id > items[1].id, "队列项 id 分配器从存储恢复且不复用");
    assert_eq!(tree(&harness, &session.id).await.session.queue_length, 3);
}

/// 规格 user story 4：首条用户消息决定标题，后续消息不再改变。
#[tokio::test]
async fn derives_auto_title_from_first_message() {
    let harness = Harness::start().await;
    let session = harness.session("workspace").await;
    assert_eq!(session.title.as_str(), agent::DEFAULT_SESSION_TITLE);

    harness.submit_message(&session.id, "第一行\n第二行").await;
    let titled = tree(&harness, &session.id).await;
    assert_eq!(titled.session.title.as_str(), "第一行");

    harness.submit_message(&session.id, "另一条消息").await;
    assert_eq!(
        tree(&harness, &session.id).await.session.title.as_str(),
        "第一行",
        "第二条消息不改变自动标题"
    );
}

/// 规格 user story 4：标题取首行前 50 个字符。
#[tokio::test]
async fn truncates_auto_title_to_fifty_characters() {
    let harness = Harness::start().await;
    let session = harness.session("workspace").await;
    let long = format!("{}\n第二行", "字".repeat(60));

    harness.submit_message(&session.id, &long).await;

    assert_eq!(
        tree(&harness, &session.id).await.session.title.as_str(),
        "字".repeat(50)
    );
}

/// 规格 user story 4：重命名后不再自动更新标题。
#[tokio::test]
async fn keeps_renamed_title_before_first_message() {
    let harness = Harness::start().await;
    let session = harness.session("workspace").await;

    let response = harness.rename_session(&session.id, "自定义标题").await;
    assert_eq!(response.status(), StatusCode::OK);

    harness.submit_message(&session.id, "首条消息").await;

    let tree = tree(&harness, &session.id).await;
    assert_eq!(tree.session.title.as_str(), "自定义标题");
    assert_eq!(tree.session.queue_length, 1);
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

    let tree = tree(&harness, &session.id).await;
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

/// 规格「删除 session」：队列随 session 一并删除。
#[tokio::test]
async fn deletes_queue_with_session() {
    let harness = Harness::start().await;
    let session = harness.session("workspace").await;
    harness.submit_message(&session.id, "待删除").await;

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

/// 规格「事实事件」：入队写入 `message_queued`，携带队列项与更新后的 session 元数据。
#[tokio::test]
async fn records_message_queued_event() {
    let harness = Harness::start().await;
    let session = harness.session("workspace").await;
    let item: agent::QueueItem = harness
        .submit_message(&session.id, "记录事件")
        .await
        .json()
        .await
        .expect("解析队列项");

    let payloads: Vec<String> = sqlx::query_scalar("SELECT event FROM event ORDER BY id")
        .fetch_all(&mut connect(&harness.database).await)
        .await
        .expect("查询事件");
    let event: agent::FactEvent =
        serde_json::from_str(payloads.last().expect("有事件")).expect("解析事件");
    let agent::FactEvent::MessageQueued {
        session: recorded,
        item: queued,
    } = event
    else {
        panic!("最后一条事件是 message_queued");
    };
    assert_eq!(queued, item);
    assert_eq!(recorded.id, session.id);
    assert_eq!(recorded.queue_length, 1);
    assert_eq!(recorded.title.as_str(), "记录事件");
    assert_eq!(recorded.updated_at, item.created_at);

    assert_eq!(
        recorded_event_kinds(&harness.database)
            .await
            .last()
            .map(String::as_str),
        Some("message_queued")
    );
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
    assert_eq!(tree(&harness, &second_session.id).await.queue, vec![second]);
}

/// ADR 0012「单一写入线」：并发入队被串行化，队列顺序与 id 顺序一致。
#[tokio::test]
async fn serializes_concurrent_submissions_into_fifo_order() {
    let harness = Harness::start().await;
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

    let tree = tree(&harness, &session.id).await;
    assert_eq!(tree.queue, items);
    assert_eq!(tree.session.queue_length, 4);
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
