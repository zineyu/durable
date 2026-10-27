//! 分支三条命令：编辑用户消息、重试 assistant、切换 active leaf 的树结果与事实序列。
//! 插话在边界的重排在 `steer_layout`；写命令入口在 `command`。

#![expect(
    clippy::expect_used,
    reason = "用例用 expect 直接报告夹具与断言失败，测试里不需要恢复。"
)]

mod support;

use durable::{
    ContentPart, FactEvent, MessageId, MessageRole, RunStatus, SessionId, Store, StoreError,
};
use support::{add_session, enqueue, facts_after, max_event_id, store_with_session, text, tree};
use tempfile::TempDir;

/// 一个已有用户节点与 assistant 回答、run 已结束的 session。
///
/// 返回临时目录、store、session、用户节点与 assistant 节点。
async fn answered() -> (TempDir, Store, SessionId, MessageId, MessageId) {
    let (directory, store, session) = store_with_session().await;
    enqueue(&store, &session, "问题").await;
    let start = store
        .start_run(&session)
        .await
        .expect("启动 run")
        .expect("队列非空时启动 run");
    let assistant = store
        .append_assistant(
            &session,
            start.run.id,
            store.mint_message_id().await.expect("铸造消息 id"),
            vec![ContentPart::text("回答")],
        )
        .await
        .expect("assistant 入树");
    store
        .finish_run(&session, &start.run, RunStatus::Succeeded, None)
        .await
        .expect("结束 run");
    let user = assistant.parent_id.expect("assistant 的父用户节点");
    (directory, store, session, user, assistant.id)
}

fn node_ids(nodes: &[durable::MessageSkeleton]) -> Vec<i64> {
    let mut ids: Vec<i64> = nodes.iter().map(|node| node.id.get()).collect();
    ids.sort_unstable();
    ids
}

/// 编辑用户消息：新用户节点与原节点同父，active leaf 移到新节点，并入队一条锚定它的项。
#[tokio::test]
async fn edit_user_message_creates_sibling_and_queues_it() {
    let (_directory, store, session, _user, assistant) = answered().await;
    enqueue(&store, &session, "第二个问题").await;
    let start = store
        .start_run(&session)
        .await
        .expect("启动 run")
        .expect("队列非空时启动 run");
    let target = tree(&store, &session)
        .await
        .active_leaf
        .expect("run 的用户节点");
    store
        .finish_run(&session, &start.run, RunStatus::Succeeded, None)
        .await
        .expect("结束 run");

    let before = max_event_id(&store).await;
    let original = tree(&store, &session).await;
    assert_eq!(
        original
            .nodes
            .iter()
            .find(|node| node.id == target)
            .expect("目标节点在树上")
            .parent_id,
        Some(assistant),
        "编辑目标不是根节点"
    );

    let edited = store
        .edit_user_message(&session, target, text("改写"))
        .await
        .expect("编辑用户消息");
    let after = tree(&store, &session).await;

    assert_eq!(edited.parent_id, Some(assistant), "新节点与原节点同父");
    assert_eq!(after.active_leaf, Some(edited.id), "active leaf 移到新节点");
    assert!(
        after.nodes.iter().any(|node| node.id == target),
        "原节点留在树上"
    );
    assert_eq!(
        after.nodes.len(),
        original.nodes.len() + 1,
        "只新增一个用户节点"
    );
    assert_eq!(after.queue.len(), 1, "写入一条队列项");
    assert_eq!(after.queue[0].anchor, Some(edited.id), "队列项锚定新节点");
    assert_eq!(after.session.queue_length, 1, "队列长度为一");
    assert!(!after.session.busy, "编辑不启动 run");

    let facts = facts_after(&store, before).await;
    assert_eq!(facts.len(), 3, "入树、移动叶子、入队各一条事实");
    assert!(
        matches!(&facts[0], FactEvent::MessageAppended { message, .. } if message.id == edited.id)
    );
    assert!(
        matches!(&facts[1], FactEvent::ActiveLeafChanged { active_leaf: Some(id), .. } if *id == edited.id)
    );
    assert!(
        matches!(&facts[2], FactEvent::MessageQueued { item, .. } if item.anchor == Some(edited.id))
    );
}

/// 重试 assistant：不创建消息节点，只入队一条锚定其父用户节点的项；随后的 run 建同父兄弟。
#[tokio::test]
async fn retry_assistant_message_queues_parent_without_new_node() {
    let (_directory, store, session, user, assistant) = answered().await;
    let before = max_event_id(&store).await;
    let original = tree(&store, &session).await;

    store
        .retry_assistant_message(&session, assistant)
        .await
        .expect("重试 assistant");
    let after = tree(&store, &session).await;

    assert_eq!(
        node_ids(&after.nodes),
        node_ids(&original.nodes),
        "重试不创建消息节点"
    );
    assert_eq!(after.queue.len(), 1, "写入一条队列项");
    assert_eq!(after.queue[0].anchor, Some(user), "锚定父用户节点");
    assert_eq!(
        after.active_leaf, original.active_leaf,
        "重试不移动 active leaf"
    );

    let facts = facts_after(&store, before).await;
    assert_eq!(facts.len(), 1, "只写入入队事实");
    assert!(matches!(&facts[0], FactEvent::MessageQueued { .. }));

    let start = store
        .start_run(&session)
        .await
        .expect("启动 run")
        .expect("队列非空时启动 run");
    let retried = store
        .append_assistant(
            &session,
            start.run.id,
            store.mint_message_id().await.expect("铸造消息 id"),
            vec![ContentPart::text("重答")],
        )
        .await
        .expect("assistant 入树");
    assert_eq!(retried.parent_id, Some(user), "新回答与原回答同父");
    let tree = tree(&store, &session).await;
    assert!(
        tree.nodes.iter().any(|node| node.id == assistant),
        "被重试的节点留在树上"
    );
    assert!(
        tree.nodes.iter().any(|node| node.id == retried.id),
        "新回答入树"
    );
}

/// 重试没有用户祖先的 assistant 时以 `Invariant` 失败。
#[tokio::test]
async fn retry_assistant_message_requires_a_user_ancestor() {
    let (_directory, store, session) = store_with_session().await;
    // 消息行引用 run 行；借另一个 session 的真实 run 满足外键，本 session 的链保持从空开始。
    let other = add_session(&store).await;
    enqueue(&store, &other, "别的 session").await;
    let run = store
        .start_run(&other)
        .await
        .expect("启动 run")
        .expect("队列非空时启动 run")
        .run
        .id;

    let rootless = store
        .append_assistant(
            &session,
            run,
            store.mint_message_id().await.expect("铸造消息 id"),
            vec![ContentPart::text("没有父节点")],
        )
        .await
        .expect("assistant 入树");
    let error = store
        .retry_assistant_message(&session, rootless.id)
        .await
        .expect_err("缺少父节点时必须失败");
    assert!(
        matches!(error, StoreError::Invariant(_)),
        "缺少父节点是 Invariant，实际为 {error:?}"
    );

    store
        .append_compaction(
            &session,
            store.mint_message_id().await.expect("铸造消息 id"),
            vec![ContentPart::text("摘要")],
        )
        .await
        .expect("压缩入树");
    let orphan = store
        .append_assistant(
            &session,
            run,
            store.mint_message_id().await.expect("铸造消息 id"),
            vec![ContentPart::text("压缩之后")],
        )
        .await
        .expect("assistant 入树");
    let error = store
        .retry_assistant_message(&session, orphan.id)
        .await
        .expect_err("父链没有用户消息时必须失败");
    assert!(
        matches!(error, StoreError::Invariant(_)),
        "父链没有用户消息是 Invariant，实际为 {error:?}"
    );
}

/// 切换 active leaf：移到目标子树中 id 最大的叶子，消息节点集合不变，只写叶子变化事实。
#[tokio::test]
async fn switch_active_leaf_moves_to_subtree_leaf_without_touching_nodes() {
    let (_directory, store, session, _user, assistant) = answered().await;
    enqueue(&store, &session, "第二个问题").await;
    let start = store
        .start_run(&session)
        .await
        .expect("启动 run")
        .expect("队列非空时启动 run");
    let second = store
        .append_assistant(
            &session,
            start.run.id,
            store.mint_message_id().await.expect("铸造消息 id"),
            vec![ContentPart::text("第二个回答")],
        )
        .await
        .expect("assistant 入树");
    store
        .finish_run(&session, &start.run, RunStatus::Succeeded, None)
        .await
        .expect("结束 run");
    let user = second.parent_id.expect("assistant 的父用户节点");

    store
        .edit_user_message(&session, user, text("改写第二个问题"))
        .await
        .expect("编辑用户消息");
    let start = store
        .start_run(&session)
        .await
        .expect("启动 run")
        .expect("队列非空时启动 run");
    let alternate = store
        .append_assistant(
            &session,
            start.run.id,
            store.mint_message_id().await.expect("铸造消息 id"),
            vec![ContentPart::text("另一个回答")],
        )
        .await
        .expect("assistant 入树");
    store
        .finish_run(&session, &start.run, RunStatus::Succeeded, None)
        .await
        .expect("结束 run");

    let before = tree(&store, &session).await;
    assert_eq!(
        before.active_leaf,
        Some(alternate.id),
        "编辑产生的分支是 active leaf"
    );

    let leaf = store
        .switch_active_leaf(&session, user)
        .await
        .expect("切换 active leaf");
    assert_eq!(leaf, second.id, "目标子树只有一片叶子时回到它");
    let switched = tree(&store, &session).await;
    assert_eq!(
        switched.active_leaf,
        Some(second.id),
        "切换后 active leaf 是第二片叶子"
    );
    assert_eq!(
        node_ids(&switched.nodes),
        node_ids(&before.nodes),
        "切换不改动消息节点集合"
    );
    let facts = facts_after(&store, before.max_event_id).await;
    assert_eq!(facts.len(), 1, "只写一条事实");
    assert!(
        matches!(&facts[0], FactEvent::ActiveLeafChanged { active_leaf: Some(id), .. } if *id == second.id)
    );

    let leaf = store
        .switch_active_leaf(&session, assistant)
        .await
        .expect("切换 active leaf");
    assert_eq!(leaf, alternate.id, "目标子树里选择 id 最大的叶子");
    let switched = tree(&store, &session).await;
    assert_eq!(
        switched.active_leaf,
        Some(alternate.id),
        "切换后 active leaf 是最新的叶子"
    );
    assert_eq!(
        node_ids(&switched.nodes),
        node_ids(&before.nodes),
        "第二次切换也不改节点集合"
    );
}

/// 三条命令在目标角色不符或不存在时返回对应错误，且不写事实。
#[tokio::test]
async fn branch_commands_reject_wrong_targets() {
    let (_directory, store, session, user, assistant) = answered().await;
    let before = max_event_id(&store).await;
    let missing = MessageId::parse(9_999).expect("合法消息 id");

    let error = store
        .edit_user_message(&session, assistant, text("改写"))
        .await
        .expect_err("编辑 assistant 必须失败");
    assert!(
        matches!(
            error,
            StoreError::MessageRoleMismatch(id, MessageRole::Assistant) if id == assistant
        ),
        "实际为 {error:?}"
    );

    let error = store
        .retry_assistant_message(&session, user)
        .await
        .expect_err("重试用户消息必须失败");
    assert!(
        matches!(
            error,
            StoreError::MessageRoleMismatch(id, MessageRole::User) if id == user
        ),
        "实际为 {error:?}"
    );

    let error = store
        .retry_assistant_message(&session, missing)
        .await
        .expect_err("重试不存在的消息必须失败");
    assert!(
        matches!(error, StoreError::MessageNotFound(_, id) if id == missing),
        "实际为 {error:?}"
    );

    let error = store
        .switch_active_leaf(&session, missing)
        .await
        .expect_err("切换到不存在的消息必须失败");
    assert!(
        matches!(error, StoreError::MessageNotFound(_, id) if id == missing),
        "实际为 {error:?}"
    );

    assert_eq!(max_event_id(&store).await, before, "失败的命令不写事实");
}
