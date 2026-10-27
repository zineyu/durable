//! 流式进度：run 的 progress 写入、定型与清空同一事务、崩溃与取消时入树为已中断消息。
//! 写命令入口在 `command`，run 生命周期在 `run`，消息骨架字段在 `read_path`。

#![expect(
    clippy::expect_used,
    reason = "内核测试用 expect 直接报告夹具与断言失败。"
)]

mod support;

use durable::{ContentPart, MessageRole, RunFailure, RunStatus, SessionTreeSnapshot, Store};
use support::{enqueue, facts_after, max_event_id, store_with_session, tree};

/// 启动一次 run，返回 run id 与本次 run 的用户节点 id。
async fn start(store: &Store, session: &durable::SessionId) -> (durable::RunId, i64) {
    enqueue(store, session, "做点事").await;
    let start = store
        .start_run(session)
        .await
        .expect("启动 run")
        .expect("队列非空时启动 run");
    let transcript = store.transcript(session).await.expect("读取 transcript");
    let leaf = transcript.last().expect("run 起点节点").id.get();
    (start.run.id, leaf)
}

fn progress_of(tree: &SessionTreeSnapshot) -> Option<&str> {
    tree.runs
        .iter()
        .find(|run| run.status == RunStatus::Running)
        .and_then(|run| run.progress.as_deref())
}

#[tokio::test]
async fn progress_is_readable_while_the_run_is_running() {
    let (_directory, store, session) = store_with_session().await;
    let (run, _leaf) = start(&store, &session).await;

    store
        .write_progress(&session, run, "已经写了一半")
        .await
        .expect("写入进度");

    let running = store
        .running_run(&session)
        .await
        .expect("读取 running run")
        .expect("run 仍在执行");
    assert_eq!(running.progress.as_deref(), Some("已经写了一半"));
    assert_eq!(
        progress_of(&tree(&store, &session).await),
        Some("已经写了一半")
    );
}

#[tokio::test]
async fn unchanged_progress_commits_nothing() {
    let (_directory, store, session) = store_with_session().await;
    let (run, _leaf) = start(&store, &session).await;
    store
        .write_progress(&session, run, "已经写了一半")
        .await
        .expect("写入进度");
    let before = max_event_id(&store).await;
    let updated_at = tree(&store, &session).await.session.updated_at;

    store
        .write_progress(&session, run, "已经写了一半")
        .await
        .expect("重复写入相同的正文");

    assert_eq!(
        max_event_id(&store).await,
        before,
        "正文未变时不提交，因此没有新的事实事件"
    );
    assert_eq!(
        tree(&store, &session).await.session.updated_at,
        updated_at,
        "进度写入不推进 session 的更新时间"
    );
}

#[tokio::test]
async fn writing_progress_emits_no_facts_and_keeps_the_session_view() {
    let (_directory, store, session) = store_with_session().await;
    let (run, _leaf) = start(&store, &session).await;
    let before = max_event_id(&store).await;
    let metadata = tree(&store, &session).await.session;

    store
        .write_progress(&session, run, "第一段")
        .await
        .expect("写入进度");
    store
        .write_progress(&session, run, "第一段第二句")
        .await
        .expect("写入进度");

    assert!(
        facts_after(&store, before).await.is_empty(),
        "进度写入不登记事实事件"
    );
    let after = tree(&store, &session).await.session;
    assert_eq!(
        after.updated_at, metadata.updated_at,
        "session 更新时间不动"
    );
    assert_eq!(after.queue_length, metadata.queue_length, "队列长度不动");
    assert!(after.busy, "run 仍在执行");
}

#[tokio::test]
async fn committing_the_step_and_clearing_progress_share_one_transaction() {
    let (_directory, store, session) = store_with_session().await;
    let (run, _leaf) = start(&store, &session).await;
    store
        .write_progress(&session, run, "已经写了一半")
        .await
        .expect("写入进度");
    let id = store.mint_message_id().await.expect("铸造消息 id");
    store
        .append_assistant(&session, run, id, vec![ContentPart::text("已经写了一半。")])
        .await
        .expect("定型 assistant");

    assert_eq!(
        store
            .running_run(&session)
            .await
            .expect("读取 running run")
            .expect("run 仍在执行")
            .progress,
        None,
        "定型与清空进度同一事务，因此定型之后没有残留"
    );

    store
        .write_progress(&session, run, "第二段")
        .await
        .expect("写入进度");
    let failed = store
        .append_assistant(&session, run, id, vec![ContentPart::text("重复的 id")])
        .await;
    assert!(failed.is_err(), "重复的消息 id 让事务失败");
    assert_eq!(
        store
            .running_run(&session)
            .await
            .expect("读取 running run")
            .expect("run 仍在执行")
            .progress
            .as_deref(),
        Some("第二段"),
        "事务失败时进度不被清空"
    );
    let transcript = store.transcript(&session).await.expect("读取 transcript");
    assert_eq!(
        transcript
            .iter()
            .filter(|node| node.role == MessageRole::Assistant)
            .count(),
        1,
        "事务失败时不留下第二条消息"
    );
}

#[tokio::test]
async fn recovery_turns_progress_into_an_interrupted_message() {
    let (_directory, store, session) = store_with_session().await;
    let (run, leaf) = start(&store, &session).await;
    store
        .write_progress(&session, run, "已经写了一半")
        .await
        .expect("写入进度");

    let recovered = store
        .materialize_progress(&session)
        .await
        .expect("恢复进度")
        .expect("progress 非空时入树");

    assert_eq!(recovered.role, MessageRole::Assistant);
    assert!(recovered.interrupted, "入树的消息标注已中断");
    assert_eq!(recovered.parent_id.map(durable::MessageId::get), Some(leaf));
    assert_eq!(recovered.text(), "已经写了一半");
    assert_eq!(
        store
            .running_run(&session)
            .await
            .expect("读取 running run")
            .expect("run 仍在执行")
            .progress,
        None,
        "入树与清空进度同一事务"
    );
    assert_eq!(
        store
            .transcript(&session)
            .await
            .expect("读取 transcript")
            .last()
            .map(|node| node.id),
        Some(recovered.id),
        "已中断消息成为新的 active leaf"
    );

    assert_eq!(
        store
            .materialize_progress(&session)
            .await
            .expect("再次恢复"),
        None,
        "进度已清空时不再写出第二条消息"
    );
    let snapshot = store.session_tree(&session).await.expect("读取消息树");
    let stored = snapshot
        .nodes
        .iter()
        .find(|node| node.id == recovered.id)
        .expect("骨架里有已中断消息");
    assert!(stored.interrupted, "骨架带上已中断标记");
}

#[tokio::test]
async fn cancelling_keeps_partial_output_and_ends_the_run_as_cancelled() {
    let (_directory, store, session) = store_with_session().await;
    let (run, _leaf) = start(&store, &session).await;
    let started = store
        .running_run(&session)
        .await
        .expect("读取 running run")
        .expect("run 仍在执行");
    store
        .write_progress(&session, run, "被取消前的半句")
        .await
        .expect("写入进度");
    store
        .materialize_progress(&session)
        .await
        .expect("入树已中断消息")
        .expect("progress 非空时入树");
    store
        .finish_run(&session, &started, RunStatus::Cancelled, None)
        .await
        .expect("结束 run");

    let transcript = store.transcript(&session).await.expect("读取 transcript");
    let last = transcript.last().expect("链尾是已中断消息");
    assert!(last.interrupted);
    assert_eq!(last.text(), "被取消前的半句");
    let finished = store
        .session_tree(&session)
        .await
        .expect("读取消息树")
        .runs
        .into_iter()
        .find(|record| record.id == run)
        .expect("run 记录");
    assert_eq!(finished.status, RunStatus::Cancelled);
    assert_eq!(finished.progress, None);
}

#[tokio::test]
async fn finishing_a_run_clears_progress_without_making_a_message() {
    let (_directory, store, session) = store_with_session().await;
    let (run, _leaf) = start(&store, &session).await;
    let started = store
        .running_run(&session)
        .await
        .expect("读取 running run")
        .expect("run 仍在执行");
    store
        .write_progress(&session, run, "上游失败前的半句")
        .await
        .expect("写入进度");
    store
        .finish_run(
            &session,
            &started,
            RunStatus::Failed,
            Some(RunFailure {
                code: durable::RunFailureCode::ModelUpstream,
                message: "连接被拒绝".to_owned(),
            }),
        )
        .await
        .expect("结束 run");

    let snapshot = store.session_tree(&session).await.expect("读取消息树");
    assert!(
        snapshot.nodes.iter().all(|node| !node.interrupted),
        "上游失败不产生已中断消息"
    );
    let finished = snapshot
        .runs
        .into_iter()
        .find(|record| record.id == run)
        .expect("run 记录");
    assert_eq!(finished.status, RunStatus::Failed);
    assert_eq!(finished.progress, None, "run 结束时清空进度");
}

#[tokio::test]
async fn interrupted_message_keeps_its_marker_after_a_later_step() {
    let (_directory, store, session) = store_with_session().await;
    let (run, _leaf) = start(&store, &session).await;
    store
        .write_progress(&session, run, "已经写了一半")
        .await
        .expect("写入进度");
    let recovered = store
        .materialize_progress(&session)
        .await
        .expect("恢复进度")
        .expect("progress 非空时入树");

    let id = store.mint_message_id().await.expect("铸造消息 id");
    store
        .append_assistant(&session, run, id, vec![ContentPart::text("续写完成。")])
        .await
        .expect("定型 assistant");

    let transcript = store.transcript(&session).await.expect("读取 transcript");
    assert_eq!(transcript.len(), 3, "用户、已中断、续跑各一条");
    assert!(transcript[1].interrupted, "已中断标记不随后续消息改变");
    assert_eq!(transcript[1].id, recovered.id);
    assert_eq!(
        transcript[2].parent_id,
        Some(recovered.id),
        "续跑的消息接在已中断消息之后"
    );
    assert!(!transcript[2].interrupted, "续跑的消息不带已中断标记");
}

#[tokio::test]
async fn progress_write_requires_the_run_to_be_running() {
    let (_directory, store, session) = store_with_session().await;
    let (run, _leaf) = start(&store, &session).await;
    let started = store
        .running_run(&session)
        .await
        .expect("读取 running run")
        .expect("run 仍在执行");
    store
        .finish_run(&session, &started, RunStatus::Succeeded, None)
        .await
        .expect("结束 run");

    assert!(
        store
            .write_progress(&session, run, "结束之后的进度")
            .await
            .is_err(),
        "结束的 run 不接受进度"
    );
}
