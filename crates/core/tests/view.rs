//! 视图不变量：同一批变更提交后，提交处推导的 session 视图与两处读回一致（ADR 0053）。
//! 命令入口的其余行为在 `command`，队列顺序在 `queue_order`。

#![expect(
    clippy::expect_used,
    reason = "用例用 expect 直接报告夹具与断言失败，测试里不需要恢复。"
)]

mod support;

use durable::{
    MessageRole, RunStatus, SessionId, SessionMetadata, SessionTreeSnapshot, Store, StoreError,
};
use support::{add_session, enqueue, max_event_id, open_store, steer, text, title, tree};

/// 固定种子的线性同余发生器：同一个种子给出同一条命令序列。
struct Pick(u64);

impl Pick {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn below(&mut self, limit: u64) -> u64 {
        self.next() % limit
    }
}

/// 断言读回一致，并返回读回的视图。
///
/// 三条路径都读同一个提交结果：本次提交的事实信封、`session_tree` 的轻量查询、`bootstrap`
/// 的全量查询。后两者走不同的 SQL，漂移必须在这里失败。
async fn read_back(store: &Store, session: &SessionId, after: i64, step: u64) -> SessionMetadata {
    let tree_view = store
        .session_tree(session)
        .await
        .expect("读回消息树")
        .session;
    let bootstrap = store.bootstrap().await.expect("读取快照");
    let bootstrap_view = bootstrap
        .sessions
        .iter()
        .find(|view| view.id == *session)
        .expect("快照里有这个 session");
    assert_eq!(
        bootstrap_view, &tree_view,
        "第 {step} 步：bootstrap 读回与 session_tree 读回不同"
    );
    for event in store.events_after(after).await.expect("回放事件") {
        if let Some(view) = &event.session {
            assert_eq!(view, &tree_view, "第 {step} 步：信封视图与读回不同");
        }
    }
    tree_view
}

/// 以成功结束当前 running 的 run。
async fn finish_running(store: &Store, session: &SessionId) {
    if let Some(run) = store
        .running_run(session)
        .await
        .expect("读取 running 的 run")
    {
        store
            .finish_run(session, &run, RunStatus::Succeeded, None, Some(1))
            .await
            .expect("结束 run");
    }
}

/// 编辑第一个用户消息；没有用户消息时什么都不做。
async fn edit_first_user(store: &Store, session: &SessionId, snapshot: &SessionTreeSnapshot) {
    if let Some(node) = snapshot
        .nodes
        .iter()
        .find(|node| node.role == MessageRole::User)
    {
        store
            .edit_user_message(session, node.id, text("改写"))
            .await
            .expect("编辑用户消息");
    }
}

/// 把 active leaf 移到最后一个节点；没有节点时什么都不做。
async fn switch_to_last(store: &Store, session: &SessionId, snapshot: &SessionTreeSnapshot) {
    if let Some(node) = snapshot.nodes.last() {
        store
            .switch_active_leaf(session, node.id)
            .await
            .expect("切换 active leaf");
    }
}

/// 从公开写命令的菜单里反复取值，每一步之后断言推导与读回一致。
#[tokio::test]
async fn derived_view_matches_every_read_back() {
    let (_directory, store) = open_store().await;
    let mut session = add_session(&store).await;
    let mut pick = Pick(0x5eed_5eed);

    for step in 0..120u64 {
        let before = max_event_id(&store).await;
        let snapshot = tree(&store, &session).await;
        let running = store
            .running_run(&session)
            .await
            .expect("读取 running 的 run");
        let queued = !snapshot.queue.is_empty();
        let busy = snapshot.session.busy;
        let idle = !queued && !busy;

        match pick.below(11) {
            0 | 1 => {
                enqueue(&store, &session, "排队").await;
            }
            2 => {
                steer(&store, &session, "插话").await;
            }
            3 if queued && !busy => {
                let started = store.start_run(&session).await.expect("开始 run");
                assert!(started.is_some(), "第 {step} 步：有队列项时 run 应当开始");
            }
            4 if busy => finish_running(&store, &session).await,
            5 => {
                let name = if step % 2 == 0 {
                    "改名甲"
                } else {
                    "改名乙"
                };
                let renamed = store
                    .rename_session(&session, &title(name))
                    .await
                    .expect("重命名");
                for event in store.events_after(before).await.expect("回放事件") {
                    if let Some(view) = &event.session {
                        assert_eq!(view, &renamed, "第 {step} 步：命令返回的视图与信封视图不同");
                    }
                }
            }
            6 if idle => edit_first_user(&store, &session, &snapshot).await,
            7 if idle => switch_to_last(&store, &session, &snapshot).await,
            8 if let Some(run) = &running => {
                store
                    .apply_pending_steers(&session, run.id)
                    .await
                    .expect("接入待插入的插话");
            }
            9 => {
                store
                    .downgrade_pending_steers(&session)
                    .await
                    .expect("降级待插入的插话");
            }
            10 => {
                store.delete_session(&session).await.expect("删除 session");
                for event in store.events_after(before).await.expect("回放事件") {
                    assert_eq!(event.session, None, "第 {step} 步：删除后信封视图为空");
                }
                assert!(
                    matches!(
                        store.session_tree(&session).await,
                        Err(StoreError::SessionNotFound(_))
                    ),
                    "第 {step} 步：删除后读回应当报 session 不存在"
                );
                session = add_session(&store).await;
            }
            _ => {}
        }

        read_back(&store, &session, before, step).await;
    }
}
