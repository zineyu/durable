//! 写命令入口的公开 API 用例：事件信封的 session 视图、空闲判定与提交失败的回滚。
//! 迁移回填在 `migration`；读方法与行映射在 `crates/core/src/storage`。

#![expect(clippy::expect_used, reason = "测试用 expect 直接报告夹具与断言失败。")]

use durable::{
    CommittedEvent, ContentPart, FactEvent, HostKind, HostName, MessageText, RunId, RunRecord,
    RunStatus, SessionId, SessionTitle, Store, StoreError, SubmitMode, WorkspaceMetadata,
    WorkspacePath,
};
use tempfile::TempDir;

/// 打开临时库，注册 workspace 并创建一个 session。
async fn store_with_session() -> (TempDir, Store, SessionId) {
    let directory = tempfile::tempdir().expect("创建临时目录");
    let store = Store::open(&directory.path().join("store.sqlite"))
        .await
        .expect("打开数据库");
    let host = HostName::parse("local").expect("合法 host 名");
    let workspace = WorkspacePath::parse("/work").expect("合法 workspace 路径");
    store
        .register_host(host.clone(), HostKind::Local, None)
        .await
        .expect("注册 host");
    store
        .register_workspace(&host, &workspace)
        .await
        .expect("注册 workspace");
    let title = SessionTitle::parse("初始标题").expect("合法标题");
    let session = store
        .create_session(&host, &workspace, title)
        .await
        .expect("创建 session");
    (directory, store, session.id)
}

fn text(raw: &str) -> MessageText {
    MessageText::parse(raw).expect("合法消息正文")
}

fn title(raw: &str) -> SessionTitle {
    SessionTitle::parse(raw).expect("合法标题")
}

/// 命令提交后的事件信封必须与提交后快照的 session 元数据逐字段相等。
async fn envelope_matches_snapshot(
    store: &Store,
    session: &SessionId,
    after: i64,
) -> Vec<CommittedEvent> {
    let committed = store.events_after(after).await.expect("回放事件");
    let tree = store.session_tree(session).await.expect("读取消息树");
    assert!(!committed.is_empty(), "写命令必须写入事实事件");
    for event in &committed {
        assert_eq!(
            event.session.as_ref(),
            Some(&tree.session),
            "事件 {} 的信封 session 必须等于提交后快照",
            event.id
        );
    }
    committed
}

async fn max_event_id(store: &Store) -> i64 {
    store.bootstrap().await.expect("读取快照").max_event_id
}

/// 提交失败时不发布事件，`event` 表不新增行。
///
/// `finish_run` 传入不存在的 run id 时 `update_run` 命中零行，内核以 `Invariant` 失败。
#[tokio::test]
async fn commit_failure_publishes_nothing() {
    let (_directory, store, session) = store_with_session().await;
    let mut receiver = store.events().subscribe();
    let before = max_event_id(&store).await;
    let missing = RunRecord {
        id: RunId::parse(999).expect("合法 run id"),
        session_id: session.clone(),
        status: RunStatus::Running,
        failure: None,
        progress: None,
        created_at: 1,
        updated_at: 1,
    };

    let error = store
        .finish_run(&session, &missing, RunStatus::Succeeded, None)
        .await
        .expect_err("run 行未命中时必须失败");
    assert!(
        matches!(error, StoreError::Invariant(_)),
        "提交失败必须是 Invariant，实际为 {error:?}"
    );

    assert_eq!(max_event_id(&store).await, before, "event 表不新增行");
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

/// 排队与 run 生命周期的命令。
#[tokio::test]
async fn queue_and_run_commands_fill_the_envelope() {
    let (_directory, store, session) = store_with_session().await;

    let mut after = max_event_id(&store).await;
    store
        .enqueue_message(&session, text("排队消息"), SubmitMode::Queue)
        .await
        .expect("入队");
    envelope_matches_snapshot(&store, &session, after).await;

    after = max_event_id(&store).await;
    let start = store
        .start_run(&session)
        .await
        .expect("启动 run")
        .expect("队列非空时启动 run");
    envelope_matches_snapshot(&store, &session, after).await;

    after = max_event_id(&store).await;
    store
        .enqueue_message(&session, text("插话"), SubmitMode::Steer)
        .await
        .expect("提交插话");
    envelope_matches_snapshot(&store, &session, after).await;

    after = max_event_id(&store).await;
    store
        .apply_pending_steers(&session, start.run.id)
        .await
        .expect("接入插话");
    envelope_matches_snapshot(&store, &session, after).await;

    store
        .enqueue_message(&session, text("第二次插话"), SubmitMode::Steer)
        .await
        .expect("提交插话");
    after = max_event_id(&store).await;
    store
        .downgrade_pending_steers(&session)
        .await
        .expect("降级插话");
    envelope_matches_snapshot(&store, &session, after).await;
    drain_queue(&store, &session).await;

    after = max_event_id(&store).await;
    store
        .finish_run(&session, &start.run, RunStatus::Succeeded, None)
        .await
        .expect("结束 run");
    envelope_matches_snapshot(&store, &session, after).await;
}

/// 消息入树的命令。
#[tokio::test]
async fn message_commands_fill_the_envelope() {
    let (_directory, store, session) = store_with_session().await;
    store
        .enqueue_message(&session, text("问题"), SubmitMode::Queue)
        .await
        .expect("入队");
    let start = store
        .start_run(&session)
        .await
        .expect("启动 run")
        .expect("队列非空时启动 run");

    let mut after = max_event_id(&store).await;
    let tool = store
        .append_tool(
            &session,
            start.run.id,
            store.mint_message_id().await.expect("铸造消息 id"),
            "call-1",
        )
        .await
        .expect("工具节点入树");
    envelope_matches_snapshot(&store, &session, after).await;

    after = max_event_id(&store).await;
    store
        .finish_tool(
            &session,
            tool.id,
            "call-1",
            durable::ToolResultState::Finished {
                outcome: durable::ToolOutcome::Succeeded {
                    content: "ok".to_owned(),
                },
            },
        )
        .await
        .expect("写入工具结局");
    envelope_matches_snapshot(&store, &session, after).await;

    after = max_event_id(&store).await;
    store
        .append_assistant(
            &session,
            start.run.id,
            store.mint_message_id().await.expect("铸造消息 id"),
            vec![ContentPart::text("回答")],
        )
        .await
        .expect("assistant 入树");
    envelope_matches_snapshot(&store, &session, after).await;

    after = max_event_id(&store).await;
    store
        .append_compaction(
            &session,
            store.mint_message_id().await.expect("铸造消息 id"),
            vec![ContentPart::text("摘要")],
        )
        .await
        .expect("压缩入树");
    envelope_matches_snapshot(&store, &session, after).await;
}

/// 要求空闲的命令与重命名。
#[tokio::test]
async fn idle_commands_fill_the_envelope() {
    let (_directory, store, session) = store_with_session().await;
    store
        .enqueue_message(&session, text("问题"), SubmitMode::Queue)
        .await
        .expect("入队");
    let start = store
        .start_run(&session)
        .await
        .expect("启动 run")
        .expect("队列非空时启动 run");
    let user = store
        .session_tree(&session)
        .await
        .expect("读取消息树")
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

    let mut after = max_event_id(&store).await;
    store
        .rename_session(&session, &title("改名后"))
        .await
        .expect("重命名");
    envelope_matches_snapshot(&store, &session, after).await;

    let item = store
        .enqueue_message(&session, text("待删除"), SubmitMode::Queue)
        .await
        .expect("入队");
    after = max_event_id(&store).await;
    store
        .remove_queue_item(&session, item.id)
        .await
        .expect("删除队列项");
    envelope_matches_snapshot(&store, &session, after).await;

    after = max_event_id(&store).await;
    store
        .edit_user_message(&session, user, text("改写"))
        .await
        .expect("编辑用户消息");
    envelope_matches_snapshot(&store, &session, after).await;
    drain_queue(&store, &session).await;

    after = max_event_id(&store).await;
    store
        .retry_assistant_message(&session, assistant.id)
        .await
        .expect("重试 assistant");
    envelope_matches_snapshot(&store, &session, after).await;
    drain_queue(&store, &session).await;

    after = max_event_id(&store).await;
    store
        .switch_active_leaf(&session, user)
        .await
        .expect("切换 active leaf");
    envelope_matches_snapshot(&store, &session, after).await;
}

/// 清空该 session 的队列，使后续命令满足空闲判定。
async fn drain_queue(store: &Store, session: &SessionId) {
    while let Some(item) = store
        .session_tree(session)
        .await
        .expect("读取消息树")
        .queue
        .first()
        .map(|item| item.id)
    {
        store
            .remove_queue_item(session, item)
            .await
            .expect("删除队列项");
    }
}

/// `create_session` 的信封 session 视图是队列 0、没有 run。
#[tokio::test]
async fn create_session_reports_an_empty_view() {
    let (_directory, store, _session) = store_with_session().await;
    let host = HostName::parse("local").expect("合法 host 名");
    let workspace = WorkspacePath::parse("/work").expect("合法 workspace 路径");
    let before = max_event_id(&store).await;

    let created = store
        .create_session(&host, &workspace, title("第二个"))
        .await
        .expect("创建 session");
    let committed = store.events_after(before).await.expect("回放事件");
    assert_eq!(committed.len(), 1);
    assert_eq!(
        committed[0].session.as_ref(),
        Some(&created),
        "信封视图就是返回值"
    );
    assert_eq!(created.queue_length, 0);
    assert!(!created.busy);
}

/// 规格 user story R-5 与 R-7：改名带走 workspace 与 session，同时更换目标会清空已安装构建。
#[tokio::test]
async fn renaming_and_retargeting_a_host_keeps_data_and_clears_the_build() {
    let (_directory, store, _session) = store_with_session().await;
    let host = HostName::parse("laptop").expect("合法 host 名");
    let path = WorkspacePath::parse("/work").expect("合法 workspace 路径");
    store
        .register_host(host.clone(), HostKind::Ssh, Some("laptop".to_owned()))
        .await
        .expect("注册 host");
    store
        .register_workspace(&host, &path)
        .await
        .expect("注册 workspace");
    let laptop_session = store
        .create_session(&host, &path, title("改名前的会话"))
        .await
        .expect("在 laptop 上创建 session");
    store
        .set_host_installed_build(&host, "0.1.0-x86_64")
        .await
        .expect("记录已安装构建");

    let renamed = HostName::parse("studio").expect("合法 host 名");
    store
        .update_host(
            &host,
            durable::HostUpdate {
                name: Some(renamed.clone()),
                target: Some("studio.lan".to_owned()),
                enabled: None,
            },
        )
        .await
        .expect("改名并更换目标");

    let snapshot = store.bootstrap().await.expect("读取快照");
    let workspaces: Vec<&WorkspaceMetadata> = snapshot
        .workspaces
        .iter()
        .filter(|workspace| workspace.path == path && workspace.host == renamed)
        .collect();
    assert_eq!(workspaces.len(), 1, "workspace 跟着改名");
    assert_eq!(workspaces[0].host, renamed, "workspace 的 host 是新的名字");
    let session = snapshot
        .sessions
        .iter()
        .find(|entry| entry.id == laptop_session.id)
        .expect("session 仍在");
    assert_eq!(session.host, renamed, "session 跟着改名");
    assert_eq!(
        store
            .host_installed_build(&renamed)
            .await
            .expect("读取构建"),
        None,
        "更换目标清空已安装构建"
    );
    assert!(
        store.host(&host).await.expect("读取旧名").is_none(),
        "旧名立即消失"
    );
}

/// 根命令的事实不带 session 视图。
#[tokio::test]
async fn root_commands_have_no_session_view() {
    let (_directory, store, _session) = store_with_session().await;
    let workspace = WorkspacePath::parse("/other").expect("合法 workspace 路径");
    let host = HostName::parse("laptop").expect("合法 host 名");

    let before = max_event_id(&store).await;
    store
        .register_host(host.clone(), HostKind::Ssh, Some("laptop".to_owned()))
        .await
        .expect("注册 host");
    store
        .register_workspace(&host, &workspace)
        .await
        .expect("注册 workspace");
    store
        .update_host(
            &host,
            durable::HostUpdate {
                name: None,
                target: Some("laptop.lan".to_owned()),
                enabled: None,
            },
        )
        .await
        .expect("更新 host");
    store
        .remove_workspace(&host, &workspace)
        .await
        .expect("移除 workspace");
    store.remove_host(&host).await.expect("移除 host");

    let committed = store.events_after(before).await.expect("回放事件");
    let kinds: Vec<&FactEvent> = committed.iter().map(|event| &event.event).collect();
    assert_eq!(kinds.len(), 5, "五条命令各写一条事实");
    assert!(
        matches!(kinds[0], FactEvent::HostRegistered { host: recorded } if recorded.name == host)
    );
    assert!(
        matches!(kinds[1], FactEvent::WorkspaceRegistered { workspace: recorded } if recorded.host == host && recorded.path == workspace)
    );
    assert!(
        matches!(kinds[2], FactEvent::HostUpdated { host: recorded } if recorded.target.as_deref() == Some("laptop.lan"))
    );
    assert_eq!(
        *kinds[3],
        FactEvent::WorkspaceRemoved {
            host: host.clone(),
            path: workspace.clone(),
        }
    );
    assert_eq!(*kinds[4], FactEvent::HostRemoved { host: host.clone() });
    assert!(
        committed.iter().all(|event| event.session.is_none()),
        "host 与 workspace 事实不带 session 视图"
    );
}

/// 删除 session 的事实没有 session 视图。
#[tokio::test]
async fn deleted_session_fact_has_no_view() {
    let (_directory, store, session) = store_with_session().await;
    let before = max_event_id(&store).await;

    store.delete_session(&session).await.expect("删除 session");
    let committed = store.events_after(before).await.expect("回放事件");
    assert_eq!(committed.len(), 1, "只留删除事实");
    assert_eq!(
        committed[0].event,
        FactEvent::SessionDeleted {
            session_id: session,
        }
    );
    assert!(committed[0].session.is_none(), "删除事实没有 session 视图");
}

/// `command_idle` 先报告队列非空，再报告 run 进行中。
#[tokio::test]
async fn command_idle_reports_queue_before_run() {
    let (_directory, store, session) = store_with_session().await;

    store
        .enqueue_message(&session, text("排队"), SubmitMode::Queue)
        .await
        .expect("入队");
    let error = store
        .command_idle(&session, async |_| Ok(()))
        .await
        .expect_err("队列非空时拒绝");
    assert!(
        matches!(error, StoreError::QueueNotEmpty(ref id) if *id == session),
        "队列非空优先，实际为 {error:?}"
    );

    store
        .start_run(&session)
        .await
        .expect("启动 run")
        .expect("队列非空时启动 run");
    store
        .enqueue_message(&session, text("run 中入队"), SubmitMode::Queue)
        .await
        .expect("入队");
    let error = store
        .command_idle(&session, async |_| Ok(()))
        .await
        .expect_err("队列非空时优先报告队列");
    assert!(
        matches!(error, StoreError::QueueNotEmpty(ref id) if *id == session),
        "队列非空优先于 run 进行中，实际为 {error:?}"
    );
    store
        .remove_queue_item(
            &session,
            store
                .session_tree(&session)
                .await
                .expect("读取消息树")
                .queue
                .first()
                .expect("队列非空")
                .id,
        )
        .await
        .expect("清空队列");
    let error = store
        .command_idle(&session, async |_| Ok(()))
        .await
        .expect_err("run 进行中时拒绝");
    assert!(
        matches!(error, StoreError::RunInProgress(ref id) if *id == session),
        "队列已空时报告 run 进行中，实际为 {error:?}"
    );
}

/// `command` 在 session 不存在时失败，且不留下事件。
#[tokio::test]
async fn command_rejects_missing_session() {
    let (_directory, store, _session) = store_with_session().await;
    let missing = SessionId::parse("missing").expect("合法 session id");
    let error = store
        .command(&missing, async |_| Ok(()))
        .await
        .expect_err("session 不存在时失败");
    assert!(matches!(error, StoreError::SessionNotFound(_)));
}

/// 未改动的 session 记录不写 `update_session`，`updated_at` 保持不变。
#[tokio::test]
async fn unchanged_session_record_keeps_updated_at() {
    let (_directory, store, session) = store_with_session().await;
    let first = store
        .rename_session(&session, &title("同名标题"))
        .await
        .expect("第一次重命名");
    let after = max_event_id(&store).await;

    let second = store
        .rename_session(&session, &title("同名标题"))
        .await
        .expect("第二次重命名");
    assert_eq!(
        second.updated_at, first.updated_at,
        "记录未改动时不推进更新时间"
    );
    envelope_matches_snapshot(&store, &session, after).await;
}
