//! core 集成测试的共享夹具：临时库、session、正文、快照读取与事件回放的短辅助。
//!
//! 每个集成测试目标是独立 crate，会单独编译本模块，因此这里允许未被该目标使用的条目。

#![expect(
    dead_code,
    clippy::expect_used,
    reason = "测试夹具用 expect 直接报告失败；本模块被每个测试目标各编译一次，未被该目标用到的条目属正常情况。"
)]

use durable::{
    ContentPart, EffectClass, EffectIntent, EffectObservation, FactEvent, HostKind, HostName,
    MessageId, MessageNode, MessageText, QueueItem, RunId, SessionId, SessionTitle,
    SessionTreeSnapshot, Store, StoreError, SubmitMode, ToolResultState, WorkspacePath,
};
use effects::token::{Abandon, Ledger, Outcome, Transition, abandon, declare, dispatch, perform};
use tempfile::TempDir;

/// `local` host 的名字。夹具里的 workspace 都登记在它名下。
pub(crate) fn local_host() -> HostName {
    HostName::parse("local").expect("合法 host 名")
}

/// 夹具里的 workspace 路径。
pub(crate) fn work_path() -> WorkspacePath {
    WorkspacePath::parse("/work").expect("合法 workspace 路径")
}

/// 打开临时库，注册 `local` host 与它名下的 `/work`。
pub(crate) async fn open_store() -> (TempDir, Store) {
    let directory = tempfile::tempdir().expect("创建临时目录");
    let store = Store::open(&directory.path().join("store.sqlite"))
        .await
        .expect("打开数据库");
    store
        .register_host(local_host(), HostKind::Local, None)
        .await
        .expect("注册 host");
    store
        .register_workspace(&local_host(), &work_path())
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
        .create_session(&local_host(), &work_path(), title("初始标题"))
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

/// 一次工具调用的意图。夹具里的工具调用都是非幂等的 `bash`。
fn tool_intent(session: &SessionId, run: RunId, message: MessageId, call_id: &str) -> EffectIntent {
    EffectIntent::tool(EffectClass::Bash, session.clone(), run, message, call_id)
}

/// 经落点表登记一个工具调用，返回新建节点的 id。
pub(crate) async fn declare_tool(
    store: &Store,
    session: &SessionId,
    run: RunId,
    call_id: &str,
) -> MessageId {
    let message = store.mint_message_id().await.expect("铸造消息 id");
    let _declared = declare(store, tool_intent(session, run, message, call_id))
        .await
        .expect("工具节点入树");
    message
}

/// 经落点表把已登记的工具调用标成已下发。重复登记不写行，因此可以直接重来一次。
pub(crate) async fn dispatch_tool(
    store: &Store,
    session: &SessionId,
    run: RunId,
    message: MessageId,
    call_id: &str,
) {
    let declared = declare(store, tool_intent(session, run, message, call_id))
        .await
        .expect("登记写入成功");
    let _dispatched = dispatch(store, declared).await.expect("下发写入成功");
}

/// 经落点表写入工具结局。
pub(crate) async fn observe_tool(
    store: &Store,
    session: &SessionId,
    run: RunId,
    message: MessageId,
    call_id: &str,
    state: ToolResultState,
) {
    let declared = declare(store, tool_intent(session, run, message, call_id))
        .await
        .expect("登记写入成功");
    let dispatched = dispatch(store, declared).await.expect("下发写入成功");
    let _outcome = perform(store, dispatched, |_token| async move {
        Outcome::Seen(EffectObservation::Tool(state))
    })
    .await
    .expect("工具结局写入成功");
}

/// 一步模型回复入树：走落点表的唯一入口，测试因此不直接调用 crate 内可见的写入方法。
pub(crate) async fn append_assistant(
    store: &Store,
    session: &SessionId,
    run: RunId,
    id: MessageId,
    parts: Vec<ContentPart>,
) -> Result<MessageNode, StoreError> {
    store
        .write(Transition::Observed(
            EffectIntent::model_step(session.clone(), run, id),
            EffectObservation::Assistant { parts },
        ))
        .await?;
    Ok(node_of(store, session, id).await)
}

/// 压缩消息入树，同样走落点表。
pub(crate) async fn append_compaction(
    store: &Store,
    session: &SessionId,
    id: MessageId,
    parts: Vec<ContentPart>,
) -> Result<MessageNode, StoreError> {
    store
        .write(Transition::Observed(
            EffectIntent::compaction(session.clone(), id),
            EffectObservation::Compaction { parts },
        ))
        .await?;
    Ok(node_of(store, session, id).await)
}

/// 读回刚刚入树的节点。写入之后它一定在 transcript 上。
async fn node_of(store: &Store, session: &SessionId, id: MessageId) -> MessageNode {
    store
        .transcript(session)
        .await
        .expect("读取 transcript")
        .into_iter()
        .find(|node| node.id == id)
        .expect("刚入树的节点在 transcript 里")
}

/// 经落点表放弃一个已登记的工具调用：它一定没有执行，结局写「结果未知」。
pub(crate) async fn lose_tool(
    store: &Store,
    session: &SessionId,
    run: RunId,
    message: MessageId,
    call_id: &str,
) {
    let declared = declare(store, tool_intent(session, run, message, call_id))
        .await
        .expect("登记写入成功");
    let _outcome = abandon(store, declared, Abandon::Cancelled)
        .await
        .expect("放弃写入成功");
}
