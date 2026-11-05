//! 工具恢复按证据强度处置：已登记一定没有执行，重跑；已下发按幂等分流（ADR 0037）。
//!
//! 回归证据（#67）：`declared_non_idempotent_tool_is_rerun_on_its_own_node` 在父提交
//! `ef7ed8fe` 上失败。旧语义在启动时把「已登记、未下发」的非幂等调用直接写成「结果未知」，
//! 它于是永不重跑；本条之后它重跑并留下结局。
//!
//! 本文件只写本条验收需要的那几个提交点。完整的崩溃前缀矩阵在 #83。

#![expect(clippy::expect_used, reason = "测试用 expect 直接报告夹具失败与误用。")]

use std::sync::Arc;
use std::time::Duration;

use agent::{
    ChatChunk, ChatMessage, ChatStream, EngineConfig, FactEvent, HostKind, HostName, MessageId,
    MessageText, ModelProvider, ProviderError, RunEngine, RunRecord, RunStatus, SessionId,
    SessionTitle, Store, StreamEvent, SubmitMode, ToolBox, ToolCall, ToolContext, ToolDefinition,
    ToolEnd, ToolOutcome, ToolResultState, WorkspacePath,
};
use async_trait::async_trait;
use durable::{ContentPart, EffectClass, EffectIntent, EffectObservation};
use effects::token::{Dispatched, Ledger, Transition, declare, dispatch};
use tokio::sync::{Mutex, broadcast};

/// tool 节点停在哪个提交点。`NoNode` 表示 assistant 已入树、tool 节点还没入树。
#[derive(Clone, Copy)]
enum Prefix {
    NoNode,
    Declared,
    Dispatched,
}

/// 记录每一次真正发出的工具调用，并按成功回答。
struct RecordingTools {
    calls: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl ToolBox for RecordingTools {
    fn definitions(&self) -> Vec<ToolDefinition> {
        durable::tool_definitions()
    }

    async fn execute(
        &self,
        call: &ToolCall,
        _context: &ToolContext,
        _dispatched: Dispatched<EffectIntent>,
    ) -> ToolEnd {
        self.calls.lock().await.push(call.id.clone());
        ToolEnd::Ended(ToolOutcome::Succeeded {
            content: "已执行".to_owned(),
        })
    }
}

/// 只回答一句话：工具批次补齐之后，续跑请求一次就收尾。
struct AnsweringProvider;

#[async_trait]
impl ModelProvider for AnsweringProvider {
    async fn stream_chat(
        &self,
        _messages: Vec<ChatMessage>,
        _tools: Vec<ToolDefinition>,
    ) -> Result<ChatStream, ProviderError> {
        Ok(Box::pin(futures_util::stream::iter(vec![Ok(
            ChatChunk::Text("收尾".to_owned()),
        )])))
    }
}

struct Fixture {
    _directory: tempfile::TempDir,
    store: Arc<Store>,
    session: SessionId,
    /// 夹具预先建好的 tool 节点；`NoNode` 时为 `None`。
    node: Option<MessageId>,
    calls: Arc<Mutex<Vec<String>>>,
}

/// 一个 running run：assistant 消息带一条 `call_name` 调用，tool 节点停在 `prefix`。
async fn fixture(call_name: &str, prefix: Prefix) -> Fixture {
    let directory = tempfile::tempdir().expect("创建临时目录");
    let store = Arc::new(
        Store::open(&directory.path().join("agent.sqlite"))
            .await
            .expect("打开数据库"),
    );
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
    let session = store
        .create_session(
            &host,
            &workspace,
            SessionTitle::parse("恢复").expect("合法标题"),
        )
        .await
        .expect("创建 session")
        .id;
    store
        .enqueue_message(
            &session,
            MessageText::parse("做点事").expect("合法消息正文"),
            SubmitMode::Queue,
        )
        .await
        .expect("入队");
    let start = store
        .start_run(&session)
        .await
        .expect("启动 run")
        .expect("队列里有该 run");
    let assistant = store.mint_message_id().await.expect("分配消息 id");
    store
        .write(Transition::Observed(
            EffectIntent::model_step(session.clone(), start.run.id, assistant),
            EffectObservation::Assistant {
                parts: vec![ContentPart::tool_call("call_1", call_name, "{}")],
            },
        ))
        .await
        .expect("写入带调用的 assistant 消息");

    let node = match prefix {
        Prefix::NoNode => None,
        Prefix::Declared | Prefix::Dispatched => {
            let message = store.mint_message_id().await.expect("分配消息 id");
            let class = EffectClass::of_tool(call_name).expect("已登记的工具名");
            let intent =
                EffectIntent::tool(class, session.clone(), start.run.id, message, "call_1");
            let declared = declare(store.as_ref(), intent).await.expect("登记写入成功");
            if matches!(prefix, Prefix::Dispatched) {
                let _dispatched = dispatch(store.as_ref(), declared)
                    .await
                    .expect("下发写入成功");
            }
            Some(message)
        }
    };

    Fixture {
        _directory: directory,
        store,
        session,
        node,
        calls: Arc::new(Mutex::new(Vec::new())),
    }
}

/// 一个 tool 节点的三样事实：节点 id、调用 id 与结局状态。
struct ToolNode {
    id: MessageId,
    call_id: String,
    state: ToolResultState,
}

/// 启动恢复，等 run 结束，返回全部 tool 节点、真正执行过的调用与 run 记录。
async fn recover(fixture: Fixture) -> (Vec<ToolNode>, Vec<String>, RunRecord) {
    let mut events = fixture.store.events().subscribe();
    let engine = RunEngine::new(
        Arc::clone(&fixture.store),
        Arc::new(AnsweringProvider),
        Arc::new(RecordingTools {
            calls: Arc::clone(&fixture.calls),
        }),
        EngineConfig::default(),
    );
    engine.recover().await.expect("启动恢复");
    let run = wait_for_run_finished(&mut events, &fixture.session).await;

    let transcript = fixture
        .store
        .transcript(&fixture.session)
        .await
        .expect("读取 transcript");
    let nodes: Vec<ToolNode> = transcript
        .iter()
        .flat_map(|node| {
            node.tool_results().map(|(call_id, state)| ToolNode {
                id: node.id,
                call_id: call_id.to_owned(),
                state: state.clone(),
            })
        })
        .collect();
    let calls = fixture.calls.lock().await.clone();
    (nodes, calls, run)
}

/// 只要有一个未观测的工具结局，界面就会残留「执行中」。
fn assert_no_unobserved(nodes: &[ToolNode]) {
    for node in nodes {
        assert!(
            !node.state.is_unobserved(),
            "调用 {} 的结局未观测: {:?}",
            node.call_id,
            node.state
        );
    }
}

/// 唯一的一条调用留下了结局。
fn assert_single_finished(nodes: &[ToolNode]) -> MessageId {
    assert_eq!(nodes.len(), 1, "一条调用对应一个 tool 节点");
    let node = nodes.first().expect("上一条断言保证有一个节点");
    assert_eq!(node.call_id, "call_1", "唯一的节点属于这条调用");
    assert!(
        matches!(node.state, ToolResultState::Finished { .. }),
        "调用 call_1 应留下结局: {:?}",
        node.state
    );
    node.id
}

/// assistant 已入树、tool 节点还没入树：重跑，并新建节点记下结局。
#[tokio::test]
async fn tool_without_a_node_is_rerun_with_a_new_node() {
    let (nodes, calls, run) = recover(fixture("bash", Prefix::NoNode).await).await;

    assert_eq!(run.status, RunStatus::Succeeded);
    assert_eq!(calls, vec!["call_1".to_owned()], "未入树的调用被执行一次");
    assert_single_finished(&nodes);
    assert_no_unobserved(&nodes);
}

/// #67 的原始用例：已登记未下发的非幂等调用重跑，并在它自己的节点上留下结局。
///
/// 父提交 `ef7ed8fe` 上这条用例失败，失败行是「已登记未下发的调用一定没有执行，
/// 因此重跑」，实测 `left: []`、`right: ["call_1"]`：旧实现启动时先把结局写成
/// 「结果未知」，`plan` 于是认为该调用已完成，`bash` 永不执行。
#[tokio::test]
async fn declared_non_idempotent_tool_is_rerun_on_its_own_node() {
    let fixture = fixture("bash", Prefix::Declared).await;
    let node = fixture.node.expect("夹具建好了 tool 节点");
    let (nodes, calls, run) = recover(fixture).await;

    assert_eq!(run.status, RunStatus::Succeeded);
    assert_eq!(
        calls,
        vec!["call_1".to_owned()],
        "已登记未下发的调用一定没有执行，因此重跑"
    );
    assert_eq!(
        assert_single_finished(&nodes),
        node,
        "重跑写回它自己的节点，不新建节点"
    );
    assert_no_unobserved(&nodes);
}

/// 已下发未观测的非幂等调用不重跑，结局写「结果未知」。
#[tokio::test]
async fn dispatched_non_idempotent_tool_is_not_rerun() {
    let (nodes, calls, run) = recover(fixture("bash", Prefix::Dispatched).await).await;

    assert_eq!(run.status, RunStatus::Succeeded);
    assert!(calls.is_empty(), "非幂等调用可能已经执行，因此不重跑");
    assert_eq!(nodes.len(), 1, "一条调用对应一个 tool 节点");
    let node = nodes.first().expect("上一条断言保证有一个节点");
    assert_eq!(node.state, ToolResultState::Unknown, "结局记为结果未知");
    assert_no_unobserved(&nodes);
}

/// 已下发未观测的幂等调用重跑，并覆盖结局。
#[tokio::test]
async fn dispatched_idempotent_tool_is_rerun() {
    let (nodes, calls, run) = recover(fixture("read_file", Prefix::Dispatched).await).await;

    assert_eq!(run.status, RunStatus::Succeeded);
    assert_eq!(calls, vec!["call_1".to_owned()], "幂等调用重跑并覆盖结局");
    assert_single_finished(&nodes);
    assert_no_unobserved(&nodes);
}

/// 等该 session 的 run 结束，返回它的记录。
async fn wait_for_run_finished(
    events: &mut broadcast::Receiver<StreamEvent>,
    session: &SessionId,
) -> RunRecord {
    loop {
        let event = tokio::time::timeout(Duration::from_secs(10), events.recv())
            .await
            .expect("等待 run 结束超时")
            .expect("事件流未关闭");
        if let StreamEvent::Fact {
            event: FactEvent::RunFinished { session_id, run },
            ..
        } = event
            && session_id == *session
        {
            return *run;
        }
    }
}
