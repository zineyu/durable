//! 插话布局：步骤边界接入把插话接到链尾，降级把待插入项变回排队项并保留节点。
//! 队列长度与出队顺序在 `queue_order`；写命令入口在 `command`。

#![expect(
    clippy::expect_used,
    reason = "用例用 expect 直接报告夹具与断言失败，测试里不需要恢复。"
)]

mod support;

use durable::{ContentPart, FactEvent, MessageId, MessageSkeleton, SessionId, Store};
use support::{chain_ids, enqueue, facts_after, max_event_id, steer, store_with_session, tree};

/// 树上节点的骨架。
async fn node(store: &Store, session: &SessionId, id: MessageId) -> MessageSkeleton {
    tree(store, session)
        .await
        .nodes
        .into_iter()
        .find(|node| node.id == id)
        .expect("节点在树上")
}

/// 一个 running 的 run 上的插话链：两条 assistant 消息，插话挂在第一条之后。
struct SteeredChain {
    store: Store,
    session: SessionId,
    first: MessageId,
    steered: MessageId,
    later: MessageId,
}

async fn steered_chain() -> (tempfile::TempDir, SteeredChain) {
    let (directory, store, session) = store_with_session().await;
    enqueue(&store, &session, "问题").await;
    let start = store
        .start_run(&session)
        .await
        .expect("启动 run")
        .expect("队列非空时启动 run");
    let first = store
        .append_assistant(
            &session,
            start.run.id,
            store.mint_message_id().await.expect("铸造消息 id"),
            vec![ContentPart::text("第一步")],
        )
        .await
        .expect("assistant 入树");
    let item = steer(&store, &session, "插话").await;
    let steered = item.anchor.expect("插话锚定节点");
    let later = store
        .append_assistant(
            &session,
            start.run.id,
            store.mint_message_id().await.expect("铸造消息 id"),
            vec![ContentPart::text("第二步")],
        )
        .await
        .expect("assistant 入树");

    assert_eq!(
        node(&store, &session, steered).await.parent_id,
        Some(first.id),
        "插话在提交时接在链尾"
    );
    assert_eq!(
        node(&store, &session, later.id).await.parent_id,
        Some(steered),
        "随后的回答挂在插话之下"
    );
    (
        directory,
        SteeredChain {
            store,
            session,
            first: first.id,
            steered,
            later: later.id,
        },
    )
}

/// 步骤边界接入：已产生的链上子节点改挂到最近的非插话祖先，插话接到非插话链尾之后。
#[tokio::test]
async fn apply_pending_steers_moves_steer_to_the_chain_tail() {
    let (_directory, chain) = steered_chain().await;
    let SteeredChain {
        store,
        session,
        first,
        steered,
        later,
    } = chain;
    let before = max_event_id(&store).await;
    let queued = tree(&store, &session).await;
    let item = queued.queue.first().expect("有一条待插入项").id;
    let chain = chain_ids(&store, &session).await;
    let start = store
        .running_run(&session)
        .await
        .expect("读取 running 的 run")
        .expect("run 进行中");

    let applied = store
        .apply_pending_steers(&session, start.id)
        .await
        .expect("接入插话");

    assert_eq!(applied, vec![steered], "返回接入的插话节点");
    assert_eq!(
        node(&store, &session, later).await.parent_id,
        Some(first),
        "子节点改挂到最近的非插话祖先"
    );
    assert_eq!(
        node(&store, &session, steered).await.parent_id,
        Some(later),
        "插话接在非插话链尾之后"
    );
    assert_eq!(
        node(&store, &session, steered).await.run_id,
        Some(start.id),
        "接入的插话绑定当前 run"
    );
    let after = tree(&store, &session).await;
    assert_eq!(
        after.active_leaf,
        Some(steered),
        "active leaf 移到最后一个插话"
    );
    assert!(after.queue.is_empty(), "接入会消费队列项");
    let reordered = chain_ids(&store, &session).await;
    assert_eq!(reordered.len(), chain.len(), "链上节点总数不变");
    assert_eq!(&reordered[..2], &chain[..2], "插话之前的链保持原样");
    assert_eq!(
        (&reordered[2], &reordered[3]),
        (&chain[3], &chain[2]),
        "回答与插话交换了位置"
    );

    let facts = facts_after(&store, before).await;
    assert_eq!(facts.len(), 5, "拆出、移动、消费、接入、移动叶子");
    assert!(
        matches!(&facts[0], FactEvent::MessageUpdated { message, .. } if message.id == later && message.parent_id == Some(first))
    );
    assert!(
        matches!(&facts[1], FactEvent::MessageUpdated { message, .. } if message.id == steered && message.parent_id == Some(later))
    );
    assert!(matches!(&facts[2], FactEvent::QueueItemRemoved { item_id, .. } if *item_id == item));
    assert!(
        matches!(&facts[3], FactEvent::SteerApplied { message_id, run_id, .. } if *message_id == steered && *run_id == start.id)
    );
    assert!(
        matches!(&facts[4], FactEvent::ActiveLeafChanged { active_leaf: Some(id), .. } if *id == steered)
    );
}

/// 降级：队列项变回普通排队项，插话节点留在链上原来的位置。
#[tokio::test]
async fn downgrade_keeps_node_and_marks_item_queued() {
    let (_directory, store, session) = store_with_session().await;
    enqueue(&store, &session, "问题").await;
    let start = store
        .start_run(&session)
        .await
        .expect("启动 run")
        .expect("队列非空时启动 run");
    let first = store
        .append_assistant(
            &session,
            start.run.id,
            store.mint_message_id().await.expect("铸造消息 id"),
            vec![ContentPart::text("第一步")],
        )
        .await
        .expect("assistant 入树");
    let item = steer(&store, &session, "插话").await;
    let steered = item.anchor.expect("插话锚定节点");
    assert_eq!(
        node(&store, &session, steered).await.parent_id,
        Some(first.id),
        "插话接在链尾"
    );
    let before = max_event_id(&store).await;

    let downgraded = store
        .downgrade_pending_steers(&session)
        .await
        .expect("降级插话");

    assert_eq!(downgraded.len(), 1, "一条待插入项被降级");
    assert!(!downgraded[0].pending_steer, "队列项不再是待插入项");
    assert_eq!(downgraded[0].anchor, Some(steered), "锚点保持不变");
    assert_eq!(
        node(&store, &session, steered).await.parent_id,
        Some(first.id),
        "节点留在链上原来的位置"
    );
    let after = tree(&store, &session).await;
    assert_eq!(
        after.active_leaf,
        Some(steered),
        "active leaf 是最后一个插话"
    );
    assert_eq!(after.queue.len(), 1, "队列项保留下来");
    assert_eq!(after.queue[0].id, item.id, "保留的是同一个队列项");
    assert!(!after.queue[0].pending_steer, "队列项已降级");

    let facts = facts_after(&store, before).await;
    assert_eq!(facts.len(), 2, "重写节点位置与降级事实");
    assert!(
        matches!(&facts[0], FactEvent::MessageUpdated { message, .. } if message.id == steered)
    );
    assert!(
        matches!(&facts[1], FactEvent::SteerDowngraded { item, .. } if item.id == downgraded[0].id && !item.pending_steer)
    );
}

/// 没有待插入项时两条命令都返回空，且不写事实。
#[tokio::test]
async fn steer_layout_is_a_noop_without_pending_items() {
    let (_directory, store, session) = store_with_session().await;
    let before = max_event_id(&store).await;
    let run = durable::RunId::parse(1).expect("合法 run id");

    assert!(
        store
            .apply_pending_steers(&session, run)
            .await
            .expect("接入插话")
            .is_empty(),
        "没有待插入项时返回空"
    );
    assert!(
        store
            .downgrade_pending_steers(&session)
            .await
            .expect("降级插话")
            .is_empty(),
        "没有待插入项时返回空"
    );
    assert_eq!(max_event_id(&store).await, before, "两条命令都不写事实");
}
