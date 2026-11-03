//! seam 六（ADR 0049）：真实 `Store` 与临时 SQLite，加替身执行器，直接驱动 `RunEngine`。
//!
//! 规格 user story R-36：link 断开时 run 以 `host_unreachable` 结束。
//! 规格 user story R-37：那次调用的结局是「结果未知」。
//! 本测试补足其余调用：同一批次里没有执行过的调用同样写「结果未知」。

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "测试用 expect 与 panic 直接报告夹具失败与误用。"
)]

use std::sync::Arc;
use std::time::Duration;

use agent::{
    ChatMessage, ChatStream, EngineConfig, FactEvent, HostKind, HostName, LinkClosed, MessageText,
    ModelProvider, ProviderError, RunEngine, RunFailureCode, RunRecord, RunStatus, SessionId,
    SessionTitle, Store, StreamEvent, SubmitMode, ToolBox, ToolCall, ToolContext, ToolDefinition,
    ToolEnd, ToolResultState, WorkspacePath,
};
use async_trait::async_trait;
use durable::ContentPart;
use tokio::sync::broadcast;

/// 只回答宿主丢失的执行端。
struct GoneTools;

#[async_trait]
impl ToolBox for GoneTools {
    fn definitions(&self) -> Vec<ToolDefinition> {
        durable::tool_definitions()
    }

    async fn execute(&self, _call: &ToolCall, _context: &ToolContext) -> ToolEnd {
        ToolEnd::HostGone(LinkClosed::unavailable("测试：host 已离线".to_owned()))
    }
}

/// 本测试的树尾是带调用的 assistant 消息，续跑不会请求模型。
struct UnusedProvider;

#[async_trait]
impl ModelProvider for UnusedProvider {
    async fn stream_chat(
        &self,
        _messages: Vec<ChatMessage>,
        _tools: Vec<ToolDefinition>,
    ) -> Result<ChatStream, ProviderError> {
        panic!("续跑执行工具时不应请求模型");
    }
}

#[tokio::test]
async fn host_gone_writes_unknown_for_every_call_in_the_batch() {
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
            SessionTitle::parse("断链").expect("合法标题"),
        )
        .await
        .expect("创建 session")
        .id;

    store
        .enqueue_message(
            &session,
            MessageText::parse("执行三个工具").expect("合法消息正文"),
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
        .append_assistant(
            &session,
            start.run.id,
            assistant,
            vec![
                ContentPart::tool_call("call_a", "read_file", r#"{"path":"a.txt"}"#),
                ContentPart::tool_call("call_b", "read_file", r#"{"path":"b.txt"}"#),
                ContentPart::tool_call("call_c", "read_file", r#"{"path":"c.txt"}"#),
            ],
        )
        .await
        .expect("写入带调用的 assistant 消息");

    let mut events = store.events().subscribe();
    let engine = RunEngine::new(
        Arc::clone(&store),
        Arc::new(UnusedProvider),
        Arc::new(GoneTools),
        EngineConfig::default(),
    );
    engine.recover().await.expect("启动恢复");
    let run = wait_for_run_finished(&mut events, &session).await;

    assert_eq!(run.status, RunStatus::Failed, "断链让 run 失败");
    let failure = run.failure.as_ref().expect("失败的 run 带失败说明");
    assert_eq!(failure.code, RunFailureCode::HostUnreachable);
    assert!(
        failure.message.contains("host 已离线"),
        "失败说明取断链原因: {failure:?}"
    );

    let transcript = store.transcript(&session).await.expect("读取 transcript");
    let states: Vec<(&str, &ToolResultState)> = transcript
        .iter()
        .flat_map(agent::MessageNode::tool_results)
        .collect();
    assert_eq!(states.len(), 3, "每个声明过的调用都有 tool 节点");
    for (call_id, state) in states {
        assert_eq!(
            *state,
            ToolResultState::Unknown,
            "调用 {call_id} 的结局应为结果未知"
        );
    }
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
