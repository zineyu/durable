//! 读路径：transcript 的链上顺序与压缩截断、按 id 取正文、需要执行的 session 集合。
//! 写入这些读方法所读对象的地方各有自己的文件：`queue_order`、`branch_commands`、`steer_layout`。

#![expect(
    clippy::expect_used,
    reason = "用例用 expect 直接报告夹具与断言失败，测试里不需要恢复。"
)]

mod support;

use durable::{ContentPart, MessageId, MessageRole, RunStatus, SessionId, Store};
use support::{add_session, enqueue, open_store, store_with_session, text, tree};

/// 一个已完成的问答。
async fn answered(store: &Store, session: &SessionId) -> (MessageId, MessageId) {
    enqueue(store, session, "问题").await;
    let start = store
        .start_run(session)
        .await
        .expect("启动 run")
        .expect("队列非空时启动 run");
    let assistant = store
        .append_assistant(
            session,
            start.run.id,
            store.mint_message_id().await.expect("铸造消息 id"),
            vec![ContentPart::text("回答")],
        )
        .await
        .expect("assistant 入树");
    store
        .finish_run(session, &start.run, RunStatus::Succeeded, None)
        .await
        .expect("结束 run");
    (
        assistant.parent_id.expect("assistant 的父用户节点"),
        assistant.id,
    )
}

/// `transcript` 从叶子回溯到根，在压缩消息处停止。
#[tokio::test]
async fn transcript_stops_at_compaction() {
    let (_directory, store, session) = store_with_session().await;
    answered(&store, &session).await;
    let compaction = store
        .append_compaction(
            &session,
            store.mint_message_id().await.expect("铸造消息 id"),
            vec![ContentPart::text("摘要")],
        )
        .await
        .expect("压缩入树");
    enqueue(&store, &session, "后续").await;
    store
        .start_run(&session)
        .await
        .expect("启动 run")
        .expect("队列非空时启动 run");

    let transcript = store.transcript(&session).await.expect("读取 transcript");
    assert_eq!(transcript.len(), 2, "压缩之前的节点不出现");
    assert_eq!(transcript[0].id, compaction.id, "压缩消息是链首");
    assert_eq!(
        transcript[0].role,
        MessageRole::Compaction,
        "压缩消息的角色是 compaction"
    );
    assert_eq!(transcript[0].run_id, None, "压缩消息不属于 run");
    assert_eq!(transcript[1].role, MessageRole::User, "随后是新的用户节点");
    assert_eq!(
        transcript[1].parent_id,
        Some(compaction.id),
        "后续节点挂在压缩消息之下"
    );
}

/// `transcript` 跟随 active leaf，只包含当前分支，顺序从根到叶子。
#[tokio::test]
async fn transcript_follows_the_active_branch() {
    let (_directory, store, session) = store_with_session().await;
    let (user, assistant) = answered(&store, &session).await;
    let edited = store
        .edit_user_message(&session, user, text("改写"))
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

    let transcript = store.transcript(&session).await.expect("读取 transcript");
    assert_eq!(transcript.len(), 2, "只包含当前分支的两个节点");
    assert_eq!(transcript[0].id, edited.id, "链首是编辑后的用户节点");
    assert_eq!(transcript[0].parent_id, None, "链首是根");
    assert_eq!(transcript[1].id, alternate.id, "链尾是 active leaf");
    assert!(
        transcript.iter().all(|node| node.id != assistant),
        "不包含另一分支的 assistant"
    );
}

/// `message_bodies` 按 id 返回正文，与快照骨架一致；不属于该 session 的 id 被忽略。
#[tokio::test]
async fn message_bodies_match_tree_skeletons() {
    let (_directory, store, session) = store_with_session().await;
    let (user, assistant) = answered(&store, &session).await;
    let snapshot = tree(&store, &session).await;
    let ids: Vec<MessageId> = snapshot.nodes.iter().map(|node| node.id).collect();

    let bodies = store
        .message_bodies(&session, &ids)
        .await
        .expect("批量读取正文");
    assert_eq!(bodies.len(), ids.len(), "每个骨架都取到正文");
    for body in &bodies {
        let skeleton = snapshot
            .nodes
            .iter()
            .find(|node| node.id == body.id)
            .expect("正文对应的骨架存在");
        assert_eq!(body.parts.len(), skeleton.parts.len(), "正文块数与骨架一致");
    }
    assert_eq!(
        bodies
            .iter()
            .find(|body| body.id == user)
            .expect("用户消息的正文")
            .text(),
        "问题"
    );
    assert_eq!(
        bodies
            .iter()
            .find(|body| body.id == assistant)
            .expect("assistant 的正文")
            .text(),
        "回答"
    );

    let unknown = MessageId::parse(9_999).expect("合法消息 id");
    let limited = store
        .message_bodies(&session, &[user, unknown])
        .await
        .expect("批量读取正文");
    assert_eq!(limited.len(), 1, "忽略不属于该 session 的 id");
    assert_eq!(limited[0].id, user, "只返回被请求的 id");
}

fn sorted(ids: &[SessionId]) -> Vec<String> {
    let mut out: Vec<String> = ids.iter().map(|id| id.as_str().to_owned()).collect();
    out.sort();
    out
}

/// `sessions_with_work` 含队列非空或有 running run 的 session；`sessions_with_queue` 只含前者。
#[tokio::test]
async fn sessions_with_work_and_queue_select_expected_sessions() {
    let (_directory, store) = open_store().await;
    let queued = add_session(&store).await;
    let running = add_session(&store).await;
    let idle = add_session(&store).await;

    enqueue(&store, &queued, "排队").await;
    enqueue(&store, &running, "执行").await;
    store
        .start_run(&running)
        .await
        .expect("启动 run")
        .expect("队列非空时启动 run");

    let work = sorted(
        &store
            .sessions_with_work()
            .await
            .expect("读取待执行 session"),
    );
    assert_eq!(
        work,
        sorted(&[queued.clone(), running.clone()]),
        "队列非空与 run 进行中都算需要执行"
    );
    assert!(
        !work.contains(&idle.as_str().to_owned()),
        "空闲 session 不出现"
    );

    let with_queue = sorted(
        &store
            .sessions_with_queue()
            .await
            .expect("读取队列非空的 session"),
    );
    assert_eq!(with_queue, sorted(&[queued]), "只含有队列项的 session");
}
