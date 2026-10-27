//! core 集成测试的共享夹具：临时库、session、正文、快照读取与事件回放的短辅助。
//!
//! 每个集成测试目标是独立 crate，会单独编译本模块，因此这里允许未被该目标使用的条目。

#![expect(
    dead_code,
    clippy::expect_used,
    reason = "测试夹具用 expect 直接报告失败；本模块被每个测试目标各编译一次，未被该目标用到的条目属正常情况。"
)]

use durable::{
    FactEvent, MessageText, QueueItem, SessionId, SessionTitle, SessionTreeSnapshot, Store,
    SubmitMode, WorkspacePath,
};
use tempfile::TempDir;

/// 打开临时库并注册 `/work`。
pub(crate) async fn open_store() -> (TempDir, Store) {
    let directory = tempfile::tempdir().expect("创建临时目录");
    let store = Store::open(&directory.path().join("store.sqlite"))
        .await
        .expect("打开数据库");
    store
        .register_workspace(&WorkspacePath::parse("/work").expect("合法 workspace 路径"))
        .await
        .expect("注册 workspace");
    (directory, store)
}

/// 打开临时库，注册 `/work` 并创建一个 session。
pub(crate) async fn store_with_session() -> (TempDir, Store, SessionId) {
    let (directory, store) = open_store().await;
    let session = add_session(&store).await;
    (directory, store, session)
}

/// 在 `/work` 下新建一个标题为「初始标题」的 session。
pub(crate) async fn add_session(store: &Store) -> SessionId {
    store
        .create_session(
            &WorkspacePath::parse("/work").expect("合法 workspace 路径"),
            title("初始标题"),
        )
        .await
        .expect("创建 session")
        .id
}

pub(crate) fn text(raw: &str) -> MessageText {
    MessageText::parse(raw).expect("合法消息正文")
}

pub(crate) fn title(raw: &str) -> SessionTitle {
    SessionTitle::parse(raw).expect("合法标题")
}

/// 当前事件表的最大 id。
pub(crate) async fn max_event_id(store: &Store) -> i64 {
    store.bootstrap().await.expect("读取快照").max_event_id
}

/// 按提交顺序回放 `after` 之后的事实载荷。
pub(crate) async fn facts_after(store: &Store, after: i64) -> Vec<FactEvent> {
    store
        .events_after(after)
        .await
        .expect("回放事件")
        .into_iter()
        .map(|event| event.event)
        .collect()
}

pub(crate) async fn tree(store: &Store, session: &SessionId) -> SessionTreeSnapshot {
    store.session_tree(session).await.expect("读取消息树")
}

/// 以排队语义提交一条消息。
pub(crate) async fn enqueue(store: &Store, session: &SessionId, raw: &str) -> QueueItem {
    store
        .enqueue_message(session, text(raw), SubmitMode::Queue)
        .await
        .expect("入队")
}

/// 以插话语义提交一条消息。
pub(crate) async fn steer(store: &Store, session: &SessionId, raw: &str) -> QueueItem {
    store
        .enqueue_message(session, text(raw), SubmitMode::Steer)
        .await
        .expect("提交插话")
}

/// 队列项的正文，按出队顺序。
pub(crate) fn queue_texts(tree: &SessionTreeSnapshot) -> Vec<&str> {
    tree.queue
        .iter()
        .map(|item| item.text.as_str())
        .collect::<Vec<_>>()
}

/// 当前 active leaf 的链上消息 id，根在前。
pub(crate) async fn chain_ids(store: &Store, session: &SessionId) -> Vec<i64> {
    store
        .transcript(session)
        .await
        .expect("读取 transcript")
        .iter()
        .map(|node| node.id.get())
        .collect()
}
