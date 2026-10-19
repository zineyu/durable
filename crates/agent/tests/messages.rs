use reqwest::StatusCode;

mod support;

use support::{Harness, connect, error_code};

async fn tree(harness: &Harness, id: &agent::SessionId) -> agent::SessionTree {
    harness
        .session_tree(id)
        .await
        .json()
        .await
        .expect("解析消息树")
}

/// 规格「事件信封」与「消息节点」：第一条消息成为树的根，active leaf 随之移动。
#[tokio::test]
async fn appends_first_message_as_tree_root() {
    let harness = Harness::start().await;
    let session = harness.session("workspace").await;

    let response = harness.submit_message(&session.id, "检查仓库状态").await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let node: agent::MessageNode = response.json().await.expect("解析消息节点");
    assert_eq!(node.parent_id, None);
    assert_eq!(node.role, agent::MessageRole::User);
    assert_eq!(node.text.as_str(), "检查仓库状态");
    assert!(node.created_at > 0);

    let tree = tree(&harness, &session.id).await;
    assert_eq!(tree.active_leaf, Some(node.id));
    assert_eq!(tree.nodes, vec![node.clone()]);
    assert_eq!(tree.session.updated_at, node.created_at);
}

/// 规格「消息树」：后续消息以当前 active leaf 为父节点，id 全局递增。
#[tokio::test]
async fn appends_messages_along_the_active_leaf() {
    let harness = Harness::start().await;
    let session = harness.session("workspace").await;

    let first: agent::MessageNode = harness
        .submit_message(&session.id, "第一行")
        .await
        .json()
        .await
        .expect("解析第一条消息");
    let second: agent::MessageNode = harness
        .submit_message(&session.id, "第二行\n第三行")
        .await
        .json()
        .await
        .expect("解析第二条消息");

    assert_eq!(second.parent_id, Some(first.id));
    assert!(second.id > first.id, "消息 id 全局递增");
    assert_eq!(second.text.as_str(), "第二行\n第三行");

    let tree = tree(&harness, &session.id).await;
    assert_eq!(tree.active_leaf, Some(second.id));
    assert_eq!(tree.nodes, vec![first, second]);
}

/// 不同 session 共享一个消息 id 分配器。
#[tokio::test]
async fn allocates_message_ids_across_sessions() {
    let harness = Harness::start().await;
    let first_session = harness.session("first").await;
    let second_session = harness.session("second").await;

    let first: agent::MessageNode = harness
        .submit_message(&first_session.id, "第一个 session")
        .await
        .json()
        .await
        .expect("解析第一条消息");
    let second: agent::MessageNode = harness
        .submit_message(&second_session.id, "第二个 session")
        .await
        .json()
        .await
        .expect("解析第二条消息");

    assert_eq!(second.parent_id, None, "另一个 session 从根开始");
    assert!(second.id > first.id, "消息 id 全局递增");
}

/// 规格「持久化」：消息树在 agent 重启后保持，id 分配器继续递增。
#[tokio::test]
async fn restores_messages_after_restart() {
    let harness = Harness::start().await;
    let session = harness.session("workspace").await;
    let node: agent::MessageNode = harness
        .submit_message(&session.id, "重启前的消息")
        .await
        .json()
        .await
        .expect("解析消息节点");

    let harness = harness.restart().await;
    let restored = tree(&harness, &session.id).await;
    assert_eq!(restored.active_leaf, Some(node.id));
    assert_eq!(restored.nodes, vec![node.clone()]);

    let next: agent::MessageNode = harness
        .submit_message(&session.id, "重启后的消息")
        .await
        .json()
        .await
        .expect("解析新消息");
    assert_eq!(next.parent_id, Some(node.id));
    assert!(next.id > node.id, "分配器从存储恢复");
}

/// 规格「边界解析」：空白正文不产生消息，也不移动 active leaf。
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
    assert_eq!(tree.active_leaf, None);
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

/// 规格「事实事件」：消息追加写入 `message_appended`，并携带更新后的 session 元数据。
#[tokio::test]
async fn records_message_event() {
    let harness = Harness::start().await;
    let session = harness.session("workspace").await;
    let node: agent::MessageNode = harness
        .submit_message(&session.id, "记录事件")
        .await
        .json()
        .await
        .expect("解析消息节点");

    let payloads: Vec<String> = sqlx::query_scalar("SELECT event FROM event ORDER BY id")
        .fetch_all(&mut connect(&harness.database).await)
        .await
        .expect("查询事件");
    let event: agent::FactEvent =
        serde_json::from_str(payloads.last().expect("有事件")).expect("解析事件");
    let agent::FactEvent::MessageAppended {
        session: recorded,
        message,
    } = event
    else {
        panic!("最后一条事件是 message_appended");
    };
    assert_eq!(message, node);
    assert_eq!(recorded.id, session.id);
    assert_eq!(recorded.updated_at, node.created_at);
}

/// ADR 0012「单一写入线」：并发提交被串行化，消息串成一条父子链。
#[tokio::test]
async fn serializes_concurrent_submissions_into_one_chain() {
    let harness = Harness::start().await;
    let session = harness.session("workspace").await;

    let responses = tokio::join!(
        harness.submit_message(&session.id, "a"),
        harness.submit_message(&session.id, "b"),
        harness.submit_message(&session.id, "c"),
        harness.submit_message(&session.id, "d"),
    );
    let mut nodes = Vec::new();
    for response in [responses.0, responses.1, responses.2, responses.3] {
        assert_eq!(response.status(), StatusCode::CREATED);
        nodes.push(
            response
                .json::<agent::MessageNode>()
                .await
                .expect("解析消息节点"),
        );
    }
    nodes.sort_by_key(|node| node.id);

    assert_eq!(nodes[0].parent_id, None, "第一条是根");
    for pair in nodes.windows(2) {
        assert_eq!(pair[1].parent_id, Some(pair[0].id), "按写入线串成链");
    }
    let tree = tree(&harness, &session.id).await;
    assert_eq!(tree.active_leaf, Some(nodes[3].id));
    assert_eq!(tree.nodes.len(), 4);
}

/// 分配器独立于消息行：删除 session 不回收已分配的消息 id。
#[tokio::test]
async fn keeps_message_id_allocator_after_session_deletion() {
    let harness = Harness::start().await;
    let first = harness.session("first").await;
    let before: agent::MessageNode = harness
        .submit_message(&first.id, "删除前")
        .await
        .json()
        .await
        .expect("解析消息节点");

    let response = harness.delete_session(&first.id).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let second = harness.session("second").await;
    let after: agent::MessageNode = harness
        .submit_message(&second.id, "删除后")
        .await
        .json()
        .await
        .expect("解析消息节点");
    assert!(
        after.id > before.id,
        "消息 id 不复用已删除的编号: {} <= {}",
        after.id.get(),
        before.id.get()
    );
    assert_eq!(after.parent_id, None);
}

/// 规格「删除 session」：消息树与历史事件一并删除。
#[tokio::test]
async fn deletes_messages_with_session() {
    let harness = Harness::start().await;
    let session = harness.session("workspace").await;
    harness.submit_message(&session.id, "待删除").await;

    let response = harness.delete_session(&session.id).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let response = harness.session_tree(&session.id).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM message")
        .fetch_one(&mut connect(&harness.database).await)
        .await
        .expect("统计消息行");
    assert_eq!(remaining, 0, "session 删除后不保留消息行");
}
