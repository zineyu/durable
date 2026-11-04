//! 崩溃前缀矩阵的夹具：脚本化 provider、记录副作用的替身执行端、跑一次场景，
//! 以及「打开某个提交点副本并重启」。
//!
//! 崩溃点不是靠杀进程抽样，而是靠 `Store::open_recording` 留下的副本枚举：事务是原子的，
//! 一次崩溃只可能落在两次提交之间，因此全部副本就是全部崩溃点（ADR 0056）。

#![expect(
    dead_code,
    clippy::expect_used,
    reason = "测试夹具用 expect 直接报告夹具失败；本模块被每个测试目标各编译一次，未被该目标用到的条目属正常情况。"
)]

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use agent::{
    ChatChunk, ChatMessage, ChatStream, EngineConfig, ModelProvider, ProviderError, RunEngine,
    ToolBox, ToolCall, ToolContext, ToolDefinition, ToolEnd,
};
use async_trait::async_trait;
use durable::{
    EffectIntent, HostKind, InvariantViolation, MessageId, MessageText, MessageTree, SessionId,
    SessionTitle, SessionTreeSnapshot, Store, SubmitMode, ToolOutcome, ToolResultState,
    WorkspacePath,
};
use effects::token::Dispatched;
use futures_util::stream;

/// 一次模型请求的脚本。
#[derive(Debug, Clone)]
pub(crate) enum Step {
    Text(String),
    Calls(Vec<Call>),
}

/// 脚本里的一次工具调用。
#[derive(Debug, Clone)]
pub(crate) struct Call {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) arguments: String,
}

impl Call {
    pub(crate) fn new(id: &str, name: &str) -> Self {
        Self {
            id: id.to_owned(),
            name: name.to_owned(),
            arguments: "{}".to_owned(),
        }
    }
}

/// 替身执行端记录的一次副作用。`at_commits` 是发生时刻已提交的次数，
/// 用来判断它落在哪个提交点之前。
#[derive(Debug, Clone)]
pub(crate) struct Effect {
    pub(crate) call_id: String,
    pub(crate) at_commits: u64,
}

/// 按脚本回答的 provider。脚本用尽后一律回一句话，续跑因此总能收尾。
pub(crate) struct ScriptedModel {
    steps: Mutex<VecDeque<Step>>,
}

impl ScriptedModel {
    pub(crate) fn new(steps: Vec<Step>) -> Self {
        Self {
            steps: Mutex::new(steps.into()),
        }
    }

    pub(crate) fn silent() -> Self {
        Self::new(Vec::new())
    }
}

#[async_trait]
impl ModelProvider for ScriptedModel {
    async fn stream_chat(
        &self,
        _messages: Vec<ChatMessage>,
        _tools: Vec<ToolDefinition>,
    ) -> Result<ChatStream, ProviderError> {
        let step = self
            .steps
            .lock()
            .expect("脚本锁")
            .pop_front()
            .unwrap_or_else(|| Step::Text("脚本已用尽".to_owned()));
        let chunks: Vec<Result<ChatChunk, ProviderError>> = match step {
            Step::Text(text) => vec![Ok(ChatChunk::Text(text))],
            Step::Calls(calls) => calls
                .into_iter()
                .enumerate()
                .map(|(index, call)| {
                    Ok(ChatChunk::ToolCall {
                        index,
                        id: Some(call.id),
                        name: Some(call.name),
                        arguments: call.arguments,
                    })
                })
                .collect(),
        };
        Ok(Box::pin(stream::iter(chunks)))
    }
}

/// 替身执行端：每次调用都算一次副作用，并立刻成功返回。
pub(crate) struct RecordingTools {
    store: Arc<Store>,
    effects: Arc<Mutex<Vec<Effect>>>,
}

impl RecordingTools {
    pub(crate) fn new(store: Arc<Store>, effects: Arc<Mutex<Vec<Effect>>>) -> Self {
        Self { store, effects }
    }
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
        _dispatched: &Dispatched<EffectIntent>,
    ) -> ToolEnd {
        self.effects.lock().expect("副作用日志").push(Effect {
            call_id: call.id.clone(),
            at_commits: self.store.commits(),
        });
        ToolEnd::Ended(ToolOutcome::Succeeded {
            content: format!("{} 已执行", call.name),
        })
    }
}

/// 场景跑完后的世界：库、session、副作用日志与提交点副本目录。
pub(crate) struct World {
    pub(crate) directory: PathBuf,
    pub(crate) session: SessionId,
    pub(crate) effects: Vec<Effect>,
    pub(crate) prefixes: Vec<(u64, PathBuf)>,
}

impl World {
    /// 落在提交点 `prefix` 之前的副作用次数。
    pub(crate) fn effects_before(&self, call_id: &str, prefix: u64) -> usize {
        self.effects
            .iter()
            .filter(|effect| effect.call_id == call_id && effect.at_commits <= prefix)
            .count()
    }
}

/// 跑一次场景直到静止，返回世界。
pub(crate) async fn run_scenario(root: &Path, steps: Vec<Step>) -> World {
    let store = Arc::new(
        Store::open_recording(&root.join("agent.sqlite"), &root.join("prefixes"))
            .await
            .expect("打开可记录的库"),
    );
    let host = agent::HostName::parse("local").expect("合法 host 名");
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
            SessionTitle::parse("矩阵场景").expect("合法标题"),
        )
        .await
        .expect("创建 session")
        .id;

    let effects: Arc<Mutex<Vec<Effect>>> = Arc::default();
    let engine = RunEngine::new(
        Arc::clone(&store),
        Arc::new(ScriptedModel::new(steps)),
        Arc::new(RecordingTools::new(
            Arc::clone(&store),
            Arc::clone(&effects),
        )),
        EngineConfig::default(),
    );
    store
        .enqueue_message(
            &session,
            MessageText::parse("矩阵场景的第一步").expect("合法正文"),
            SubmitMode::Queue,
        )
        .await
        .expect("入队");
    engine.notify_session(session.clone()).await;
    wait_for_rest(&store, &session).await;

    let effects = effects.lock().expect("副作用日志").clone();
    World {
        directory: root.to_path_buf(),
        session,
        effects,
        prefixes: prefixes(&root.join("prefixes")),
    }
}

/// 一个提交点副本的序号与路径，按序号升序。
pub(crate) fn prefixes(directory: &Path) -> Vec<(u64, PathBuf)> {
    let mut entries: Vec<(u64, PathBuf)> = std::fs::read_dir(directory)
        .expect("读取副本目录")
        .map(|entry| {
            let path = entry.expect("目录项").path();
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let serial = name
                .trim_start_matches("prefix-")
                .trim_end_matches(".sqlite")
                .parse::<u64>()
                .expect("副本文件名带序号");
            (serial, path)
        })
        .collect();
    entries.sort_by_key(|(serial, _)| *serial);
    entries
}

/// 重启一个提交点副本后的结果。
pub(crate) struct Restart {
    /// 重启前的链（副本里已有的历史）。
    pub(crate) before: Vec<(MessageId, Vec<(String, ToolResultState)>)>,
    /// 重启前这个 session 是否还有 run 在跑：即这个提交点是否真的落在一次 run 中间。
    pub(crate) was_busy: bool,
    /// 重启后的链。
    pub(crate) chain: Vec<MessageId>,
    /// 重启后链上每个工具调用的结局。
    pub(crate) after: Vec<(String, ToolResultState)>,
    pub(crate) tree: SessionTreeSnapshot,
    pub(crate) invariants: Vec<InvariantViolation>,
    pub(crate) effects: Vec<Effect>,
}

impl Restart {
    /// 某个调用在重启后的结局。
    pub(crate) fn state_of(&self, call_id: &str) -> Option<ToolResultState> {
        self.after
            .iter()
            .find(|(id, _)| id == call_id)
            .map(|(_, state)| state.clone())
    }

    /// 某个调用在重启后被重跑的次数。
    pub(crate) fn reruns_of(&self, call_id: &str) -> usize {
        self.effects
            .iter()
            .filter(|effect| effect.call_id == call_id)
            .count()
    }
}

/// 打开提交点副本，执行启动恢复，跑到静止，返回前后的形状。
///
/// `None` 表示这个提交点早于 session 的创建：没有可续跑的 run，副本只需自洽。
pub(crate) async fn restart(db: &Path, session: &SessionId) -> Option<Restart> {
    let store = Arc::new(Store::open(db).await.expect("打开副本"));
    let before = match store.transcript(session).await {
        Ok(nodes) => nodes
            .iter()
            .map(|node| {
                (
                    node.id,
                    node.tool_results()
                        .map(|(call, state)| (call.to_owned(), state.clone()))
                        .collect(),
                )
            })
            .collect(),
        Err(durable::StoreError::SessionNotFound(_)) => {
            assert!(
                store
                    .check_invariants()
                    .await
                    .expect("检查不变量")
                    .is_empty(),
                "{} 的不变量应当自洽",
                db.display()
            );
            return None;
        }
        Err(error) => panic!("读取副本的链失败: {error}"),
    };
    let was_busy = store
        .session_tree(session)
        .await
        .expect("读取副本的消息树")
        .session
        .busy;

    let effects: Arc<Mutex<Vec<Effect>>> = Arc::default();
    let engine = RunEngine::new(
        Arc::clone(&store),
        Arc::new(ScriptedModel::silent()),
        Arc::new(RecordingTools::new(
            Arc::clone(&store),
            Arc::clone(&effects),
        )),
        EngineConfig::default(),
    );
    engine.recover().await.expect("启动恢复");
    wait_for_rest(&store, session).await;

    let tree = store.session_tree(session).await.expect("读取消息树");
    let recovered = store.transcript(session).await.expect("读取重启后的链");
    let chain: Vec<MessageId> = recovered.iter().map(|node| node.id).collect();
    let after = recovered
        .iter()
        .flat_map(|node| {
            node.tool_results()
                .map(|(call, state)| (call.to_owned(), state.clone()))
        })
        .collect();
    Some(Restart {
        before,
        was_busy,
        chain,
        after,
        invariants: store.check_invariants().await.expect("检查不变量"),
        tree,
        effects: effects.lock().expect("副作用日志").clone(),
    })
}

/// 轮询直到 session 空闲且队列为空。恢复不收敛时用例以超时失败，而不是挂住。
pub(crate) async fn wait_for_rest(store: &Store, session: &SessionId) {
    for _ in 0..500 {
        let tree = store.session_tree(session).await.expect("读取消息树");
        if !tree.session.busy && tree.queue.is_empty() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("恢复在预期时间内没有收敛");
}

/// 链上最后一个 assistant 节点的正文。
pub(crate) async fn last_assistant_text(store: &Store, session: &SessionId) -> Option<String> {
    let transcript = store.transcript(session).await.ok()?;
    let node = transcript
        .iter()
        .rev()
        .find(|node| node.role == durable::MessageRole::Assistant)?;
    let bodies = store
        .message_bodies(session, &[node.id])
        .await
        .ok()?
        .into_iter()
        .next()?;
    Some(bodies.text())
}

/// 某个节点是否仍在链上。供断言「历史只增不减」。
pub(crate) fn tree_has(tree: &SessionTreeSnapshot, id: MessageId) -> bool {
    let chain = match tree.active_leaf {
        Some(leaf) => MessageTree::new(tree.nodes.clone()).transcript(leaf),
        None => Vec::new(),
    };
    chain.iter().any(|node| node.id == id)
}
