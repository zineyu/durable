//! 事件序列与水位：一次提交内的事件 id、提交后广播、失败不落地、bootstrap 与快照水位。
//! 写命令入口的 session 视图信封在 `command`；各对象的写命令各有自己的文件。

mod support;

use durable::{ContentPart, FactEvent, MessageId, RunStatus, StoreError, StreamEvent};
use support::{enqueue, facts_after, max_event_id, store_with_session, text, tree};

/// 一次命令提交内的事件 id 连续递增，互不重复。
#[tokio::test]
async fn commit_event_ids_are_consecutive() {
    let (_directory, store, session) = store_with_session().await;
    let before = max_event_id(&store).await;
    enqueue(&store, &session, "问题").await;
    store
        .start_run(&session)
        .await
        .expect("启动 run")
        .expect("队列非空时启动 run");

    let committed = store.events_after(before).await.expect("回放事件");
    assert!(committed.len() >= 4, "启动 run 至少提交四条事实");
    for (offset, event) in committed.iter().enumerate() {
        let expected = before + i64::try_from(offset).expect("小于 i64") + 1;
        assert_eq!(event.id, expected, "事件 id 必须连续递增");
    }
    let mut ids: Vec<i64> = committed.iter().map(|event| event.id).collect();
    ids.dedup();
    assert_eq!(ids.len(), committed.len(), "事件 id 互不重复");
}

/// 提交成功后广播的每条事实与 `events_after` 读出的事件逐字段相等。
#[tokio::test]
async fn broadcast_frames_equal_replayed_events() {
    let (_directory, store, session) = store_with_session().await;
    let mut receiver = store.events().subscribe();
    let before = max_event_id(&store).await;

    enqueue(&store, &session, "问题").await;
    store
        .start_run(&session)
        .await
        .expect("启动 run")
        .expect("队列非空时启动 run");

    let committed = store.events_after(before).await.expect("回放事件");
    let expected: Vec<StreamEvent> = committed
        .iter()
        .map(|event| StreamEvent::Fact {
            id: event.id,
            session_id: event.event.session_id().cloned(),
            session: event.session.clone(),
            timestamp: event.created_at,
            event: event.event.clone(),
        })
        .collect();
    let mut frames = Vec::new();
    while let Ok(event) = receiver.try_recv() {
        frames.push(event);
    }
    assert_eq!(frames, expected, "广播帧与回放事件逐字段相等");
}

/// 失败的命令不写事件表，也不广播。
#[tokio::test]
async fn failed_command_writes_and_publishes_nothing() {
    let (_directory, store, session) = store_with_session().await;
    let mut receiver = store.events().subscribe();
    let before = max_event_id(&store).await;
    let missing = MessageId::parse(999).expect("合法消息 id");

    let error = store
        .edit_user_message(&session, missing, text("改写"))
        .await
        .expect_err("目标不存在时必须失败");
    assert!(
        matches!(error, StoreError::MessageNotFound(_, _)),
        "实际为 {error:?}"
    );

    assert_eq!(max_event_id(&store).await, before, "事件表不新增行");
    assert!(
        store
            .events_after(before)
            .await
            .expect("回放事件")
            .is_empty(),
        "失败的命令不留下事实事件"
    );
    assert!(receiver.try_recv().is_err(), "失败的命令不发布任何事件");
}

/// `bootstrap` 的水位等于事件表的最大 id，越过水位的回放为空，session 快照水位相同。
#[tokio::test]
async fn bootstrap_watermark_matches_last_committed_event() {
    let (_directory, store, session) = store_with_session().await;
    let before = max_event_id(&store).await;
    enqueue(&store, &session, "问题").await;
    let committed = store.events_after(before).await.expect("回放事件");
    let last = committed.last().expect("有提交的事实").id;

    let bootstrap = store.bootstrap().await.expect("读取快照");
    assert_eq!(bootstrap.max_event_id, last, "水位等于事件表最大 id");
    assert!(
        store
            .events_after(bootstrap.max_event_id)
            .await
            .expect("回放事件")
            .is_empty(),
        "越过水位的回放为空"
    );

    let snapshot = tree(&store, &session).await;
    assert_eq!(
        snapshot.max_event_id, bootstrap.max_event_id,
        "session 快照水位与 bootstrap 一致"
    );
}

/// run 生命周期的全部事实按固定顺序提交。
#[tokio::test]
async fn run_lifecycle_facts_keep_their_order() {
    let (_directory, store, session) = store_with_session().await;
    let before = max_event_id(&store).await;

    enqueue(&store, &session, "问题").await;
    let start = store
        .start_run(&session)
        .await
        .expect("启动 run")
        .expect("队列非空时启动 run");
    let user = tree(&store, &session)
        .await
        .active_leaf
        .expect("run 的用户节点");
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

    let facts = facts_after(&store, before).await;
    assert_eq!(facts.len(), 8, "生命周期共八条事实");
    assert!(
        matches!(&facts[0], FactEvent::MessageQueued { .. }),
        "第一条是入队事实"
    );
    assert!(
        matches!(&facts[1], FactEvent::MessageAppended { message, .. } if message.id == user),
        "run 起点先入树"
    );
    assert!(
        matches!(&facts[2], FactEvent::ActiveLeafChanged { active_leaf: Some(id), .. } if *id == user)
    );
    assert!(matches!(&facts[3], FactEvent::QueueItemRemoved { .. }));
    assert!(matches!(&facts[4], FactEvent::RunStarted { run, .. } if run.id == start.run.id));
    assert!(
        matches!(&facts[5], FactEvent::MessageAppended { message, .. } if message.id == assistant.id)
    );
    assert!(
        matches!(&facts[6], FactEvent::ActiveLeafChanged { active_leaf: Some(id), .. } if *id == assistant.id)
    );
    assert!(
        matches!(&facts[7], FactEvent::RunFinished { run, .. } if run.status == RunStatus::Succeeded)
    );
}
