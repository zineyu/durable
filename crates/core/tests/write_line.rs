//! 写入线不变量：同一个 [`Store`] 上并发的写命令互不插入，读-改-写各看到前一条的结果。
//! 写命令入口的公开 API 在 `command`；各对象的命令各有自己的文件。

mod support;

use std::sync::Arc;

use durable::{ContentPart, FactEvent, MessageRole};
use support::{enqueue, max_event_id, queue_texts, store_with_session};

/// 并发的入队命令各自提交一次，队列项数与事件条数一致，队列顺序等于提交顺序。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_enqueues_keep_every_item_once() {
    const SUBMISSIONS: usize = 8;
    let (_directory, store, session) = store_with_session().await;
    let before = max_event_id(&store).await;
    let store = Arc::new(store);

    let mut tasks = Vec::new();
    for index in 0..SUBMISSIONS {
        let store = Arc::clone(&store);
        let session = session.clone();
        tasks.push(tokio::spawn(async move {
            enqueue(&store, &session, &format!("消息 {index}")).await;
        }));
    }
    for task in tasks {
        task.await.expect("入队任务完成");
    }

    let tree = store.session_tree(&session).await.expect("读取消息树");
    assert_eq!(tree.queue.len(), SUBMISSIONS, "并发的入队不丢失队列项");
    assert_eq!(
        tree.session.queue_length,
        u32::try_from(SUBMISSIONS).expect("小于 u32"),
        "队列长度等于提交条数"
    );

    let committed = store.events_after(before).await.expect("回放事件");
    assert_eq!(committed.len(), SUBMISSIONS, "每条入队命令提交一次事实事件");
    let committed_items: Vec<i64> = committed
        .iter()
        .filter_map(|event| match &event.event {
            FactEvent::MessageQueued { item, .. } => Some(item.id.get()),
            _ => None,
        })
        .collect();
    let queue_items: Vec<i64> = tree.queue.iter().map(|item| item.id.get()).collect();
    assert_eq!(
        committed_items, queue_items,
        "队列顺序等于提交顺序；读与改没有交错"
    );
}

/// 并发追加 assistant 时，每次命令都读到上一次提交后的 active leaf，因此结果是一条链。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_appends_extend_one_chain() {
    const APPENDS: usize = 8;
    let (_directory, store, session) = store_with_session().await;
    enqueue(&store, &session, "问题").await;
    let start = store
        .start_run(&session)
        .await
        .expect("启动 run")
        .expect("队列非空时启动 run");
    let store = Arc::new(store);

    let mut tasks = Vec::new();
    for index in 0..APPENDS {
        let store = Arc::clone(&store);
        let session = session.clone();
        let run = start.run.id;
        tasks.push(tokio::spawn(async move {
            let id = store.mint_message_id().await.expect("铸造消息 id");
            store
                .append_assistant(
                    &session,
                    run,
                    id,
                    vec![ContentPart::text(format!("回复 {index}"))],
                )
                .await
                .expect("assistant 入树")
        }));
    }
    for task in tasks {
        task.await.expect("追加任务完成");
    }

    let transcript = store.transcript(&session).await.expect("读取 transcript");
    assert_eq!(transcript.len(), APPENDS + 1, "全部追加的节点都在链上");
    assert_eq!(
        transcript[0].role,
        MessageRole::User,
        "链首是 run 的用户节点"
    );
    for pair in transcript.windows(2) {
        assert_eq!(
            pair[1].parent_id,
            Some(pair[0].id),
            "节点 {} 的父节点必须是上一条消息",
            pair[1].id.get()
        );
    }
}

/// 并发写命令之后，后续命令看到全部已提交的效果。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn later_commands_see_every_committed_effect() {
    let (_directory, store, session) = store_with_session().await;
    let store = Arc::new(store);

    let mut tasks = Vec::new();
    for index in 0..5 {
        let store = Arc::clone(&store);
        let session = session.clone();
        tasks.push(tokio::spawn(async move {
            enqueue(&store, &session, &format!("消息 {index}")).await;
        }));
    }
    for task in tasks {
        task.await.expect("入队任务完成");
    }

    let seen = store
        .command(&session, async |command| Ok(command.queue_length()))
        .await
        .expect("读取队列长度");
    assert_eq!(seen, 5, "后续命令看到全部并发的入队");

    let queued = store.session_tree(&session).await.expect("读取消息树");
    let remaining: Vec<&str> = queue_texts(&queued).into_iter().skip(1).collect();
    store
        .remove_queue_item(&session, queued.queue[0].id)
        .await
        .expect("删除队首");
    let tree = store.session_tree(&session).await.expect("读取消息树");
    assert_eq!(tree.session.queue_length, 4, "删除队首后长度减一");
    assert_eq!(queue_texts(&tree), remaining, "剩下的队列项保持顺序");
}
