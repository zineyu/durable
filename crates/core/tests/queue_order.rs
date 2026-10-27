//! 队列长度、`busy` 与出队顺序：队首被取走时长度减一，run 进行中时 `busy` 为真。
//! 队列项的写入与降级在 `steer_layout`，分支命令产生的队列项在 `branch_commands`。

#![expect(
    clippy::expect_used,
    reason = "用例用 expect 直接报告夹具与断言失败，测试里不需要恢复。"
)]

mod support;

use durable::{MessageRole, RunStatus, SessionId, Store};
use support::{enqueue, queue_texts, store_with_session, tree};

/// 链尾消息的正文。
async fn tail_text(store: &Store, session: &SessionId) -> String {
    store
        .transcript(session)
        .await
        .expect("读取 transcript")
        .last()
        .expect("链非空")
        .text()
}

/// 入队增加队列长度，`start_run` 取走队首并置 `busy`，`finish_run` 清除 `busy`。
#[tokio::test]
async fn queue_length_and_busy_follow_queue_and_run() {
    let (_directory, store, session) = store_with_session().await;
    let empty = tree(&store, &session).await;
    assert_eq!(empty.session.queue_length, 0, "新 session 的队列为空");
    assert!(!empty.session.busy, "新 session 不忙");
    assert!(
        store.start_run(&session).await.expect("启动 run").is_none(),
        "空队列不启动 run"
    );

    for raw in ["第一条", "第二条", "第三条"] {
        enqueue(&store, &session, raw).await;
    }
    let queued = tree(&store, &session).await;
    assert_eq!(queued.session.queue_length, 3, "三次入队后长度为三");
    assert!(!queued.session.busy, "只入队不改变 busy");
    assert!(queued.runs.is_empty(), "入队不创建 run");

    let start = store
        .start_run(&session)
        .await
        .expect("启动 run")
        .expect("队列非空时启动 run");
    let running = tree(&store, &session).await;
    assert_eq!(running.session.queue_length, 2, "取走队首后长度减一");
    assert!(running.session.busy, "run 进行中时 busy 为真");
    assert_eq!(running.runs.len(), 1, "只创建一个 run");
    assert_eq!(
        running.runs[0].id, start.run.id,
        "快照里的 run 是刚启动的 run"
    );
    assert_eq!(
        running.runs[0].status,
        RunStatus::Running,
        "run 状态为执行中"
    );
    assert_eq!(
        store
            .running_run(&session)
            .await
            .expect("读取 running 的 run")
            .map(|run| run.id),
        Some(start.run.id)
    );

    store
        .finish_run(&session, &start.run, RunStatus::Succeeded, None)
        .await
        .expect("结束 run");
    let finished = tree(&store, &session).await;
    assert!(!finished.session.busy, "run 结束后 busy 为假");
    assert_eq!(finished.session.queue_length, 2, "结束 run 不动队列");
    assert!(
        store
            .running_run(&session)
            .await
            .expect("读取 running 的 run")
            .is_none(),
        "结束后没有 running 的 run"
    );
}

/// 连续两次 `start_run` 各只取队首一条，起点节点按出队顺序对应。
#[tokio::test]
async fn start_run_takes_one_item_in_queue_order() {
    let (_directory, store, session) = store_with_session().await;
    for raw in ["第一条", "第二条", "第三条"] {
        enqueue(&store, &session, raw).await;
    }

    let first = store
        .start_run(&session)
        .await
        .expect("启动 run")
        .expect("队列非空时启动 run");
    assert_eq!(
        queue_texts(&tree(&store, &session).await),
        vec!["第二条", "第三条"],
        "只取走队首一条"
    );
    assert_eq!(tail_text(&store, &session).await, "第一条", "起点是第一条");

    store
        .finish_run(&session, &first.run, RunStatus::Succeeded, None)
        .await
        .expect("结束 run");
    let second = store
        .start_run(&session)
        .await
        .expect("启动 run")
        .expect("队列非空时启动 run");
    assert_ne!(first.run.id, second.run.id, "第二次 run 是新 run");
    assert_eq!(
        queue_texts(&tree(&store, &session).await),
        vec!["第三条"],
        "第二次只取走队首一条"
    );
    let transcript = store.transcript(&session).await.expect("读取 transcript");
    assert_eq!(
        transcript.last().expect("链非空").text(),
        "第二条",
        "第二次起点是第二条"
    );
    assert_eq!(
        transcript
            .iter()
            .filter(|node| node.role == MessageRole::User)
            .count(),
        2,
        "每次 run 新建一个用户节点"
    );
}

/// 队列为空或已有 running 的 run 时 `start_run` 返回 `None`，且不改动队列。
#[tokio::test]
async fn start_run_ignores_empty_queue_and_running_run() {
    let (_directory, store, session) = store_with_session().await;
    assert!(
        store.start_run(&session).await.expect("启动 run").is_none(),
        "空队列返回 None"
    );

    enqueue(&store, &session, "第一条").await;
    enqueue(&store, &session, "第二条").await;
    store
        .start_run(&session)
        .await
        .expect("启动 run")
        .expect("队列非空时启动 run");
    let busy = tree(&store, &session).await;

    assert!(
        store.start_run(&session).await.expect("启动 run").is_none(),
        "已有 running 的 run 时返回 None"
    );
    let after = tree(&store, &session).await;
    assert_eq!(
        queue_texts(&after),
        queue_texts(&busy),
        "拒绝启动时不改动队列"
    );
    assert_eq!(after.runs.len(), 1, "不创建第二个 run");
}
