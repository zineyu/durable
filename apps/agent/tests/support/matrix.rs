//! 崩溃前缀矩阵的夹具：脚本化 provider、记录副作用的替身执行端、跑一个场景、把提交点副本
//! 变成崩溃点，以及从一个点重启。
//!
//! 崩溃点不是靠杀进程抽样，而是靠 `Store::open_recording` 留下的副本枚举：事务是原子的，
//! 一次崩溃只可能落在两次提交之间，因此全部副本就是全部崩溃点（ADR 0056）。
//! 遍历与逐点完备在 `effects::matrix`，逐点断言在 `support::checks`，本模块只负责
//! 「点从哪来」与「怎么重启」。

#![expect(
    dead_code,
    clippy::expect_used,
    reason = "测试夹具用 expect 直接报告场景搭建失败；本模块被每个测试目标各编译一次，未被该目标用到的条目属正常情况。"
)]

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use agent::{
    ChatChunk, ChatMessage, ChatStream, EngineConfig, ModelProvider, ProviderError, RunEngine,
    ToolBox, ToolCall, ToolContext, ToolDefinition, ToolEnd,
};
use async_trait::async_trait;
use durable::{
    EffectClass, EffectIntent, HostKind, InvariantViolation, MessageId, MessageText, MessageTree,
    SessionId, SessionTitle, SessionTreeSnapshot, Store, SubmitMode, ToolOutcome, ToolResultState,
    WorkspacePath,
};
use effects::matrix::{CrashPoint, Finding, Kind, Matrix, Report};
use effects::token::Dispatched;
use futures_util::stream;

use super::checks;

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

    fn next_step(&self) -> Step {
        self.steps
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop_front()
            .unwrap_or_else(|| Step::Text("脚本已用尽".to_owned()))
    }
}

#[async_trait]
impl ModelProvider for ScriptedModel {
    async fn stream_chat(
        &self,
        _messages: Vec<ChatMessage>,
        _tools: Vec<ToolDefinition>,
    ) -> Result<ChatStream, ProviderError> {
        let chunks: Vec<Result<ChatChunk, ProviderError>> = match self.next_step() {
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
        _dispatched: Dispatched<EffectIntent>,
    ) -> ToolEnd {
        self.effects
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(Effect {
                call_id: call.id.clone(),
                at_commits: self.store.commits(),
            });
        ToolEnd::Ended(ToolOutcome::Succeeded {
            content: format!("{} 已执行", call.name),
        })
    }
}

/// 场景脚本里一次工具调用的期望：调用 id 与幂等标记。
///
/// 幂等标记取自工具定义，不由测试另说：非幂等的上限是 1，幂等的上限是 2（ADR 0037、ADR 0052）。
pub(crate) type CallExpectation = (&'static str, bool);

/// 一个场景：脚本、引擎配置、执行端答复的正文长度，以及期望的调用。
pub(crate) struct Scenario {
    pub(crate) name: &'static str,
    /// 依次提交的用户消息，每条提交后等到静止再提交下一条。
    pub(crate) messages: Vec<&'static str>,
    pub(crate) steps: Vec<Step>,
    pub(crate) calls: Vec<CallExpectation>,
    pub(crate) config: EngineConfig,
    /// 这个场景应当真的走到「已登记、未下发」：纯文本场景走不到。
    pub(crate) expect_declared: bool,
}

impl Scenario {
    /// 一条消息、默认配置的场景。
    pub(crate) fn new(name: &'static str, steps: Vec<Step>, calls: Vec<CallExpectation>) -> Self {
        Self {
            name,
            messages: vec!["矩阵场景的第一步"],
            steps,
            calls,
            config: EngineConfig::default(),
            expect_declared: false,
        }
    }
}

/// 场景跑完后的世界：session、副作用日志、崩溃点、已提交次数与本进程走过的类别。
pub(crate) struct World {
    pub(crate) session: SessionId,
    pub(crate) effects: Vec<Effect>,
    pub(crate) prefixes: Vec<Prefix>,
    pub(crate) commits: u64,
    pub(crate) declared: Vec<EffectClass>,
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

/// 一个提交点副本，即矩阵的一个崩溃点。
#[derive(Clone, Debug)]
pub(crate) struct Prefix {
    pub(crate) scenario: &'static str,
    pub(crate) serial: u64,
    pub(crate) path: PathBuf,
}

impl CrashPoint for Prefix {
    /// 场景名加副本序号：报告里的一行直接指向磁盘上的一个文件。
    fn label(&self) -> String {
        format!("{}/prefix-{:06}", self.scenario, self.serial)
    }
}

/// 跑一个场景直到静止，返回世界。
pub(crate) async fn run_scenario(root: &Path, scenario: &Scenario) -> World {
    std::fs::create_dir_all(root).expect("建立场景目录");
    let directory = root.join("prefixes");
    let store = Arc::new(
        Store::open_recording(&root.join("agent.sqlite"), &directory)
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
        Arc::new(ScriptedModel::new(scenario.steps.clone())),
        Arc::new(RecordingTools::new(
            Arc::clone(&store),
            Arc::clone(&effects),
        )),
        scenario.config,
    );
    for message in &scenario.messages {
        store
            .enqueue_message(
                &session,
                MessageText::parse(message).expect("合法正文"),
                SubmitMode::Queue,
            )
            .await
            .expect("入队");
        engine.notify_session(session.clone()).await;
        assert!(
            wait_for_rest(&store, &session).await,
            "{}：场景本身没有在预期时间内静止",
            scenario.name
        );
    }

    let effects = effects
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    World {
        session,
        effects,
        prefixes: prefixes(&directory, scenario.name),
        commits: store.commits(),
        declared: store.effects_declared(),
    }
}

/// 副本目录里的全部崩溃点，按序号升序。
pub(crate) fn prefixes(directory: &Path, scenario: &'static str) -> Vec<Prefix> {
    let mut points: Vec<Prefix> = std::fs::read_dir(directory)
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
            Prefix {
                scenario,
                serial,
                path,
            }
        })
        .collect();
    points.sort_by_key(|prefix| prefix.serial);
    points
}

/// 一个提交点重启后的形状。早于 session 创建的副本没有树，其余字段照旧为空。
pub(crate) struct Restart {
    /// 重启前的链：每个消息 id 与它的工具调用结局。
    pub(crate) before: Vec<(MessageId, Vec<(String, ToolResultState)>)>,
    /// 重启前这个 session 是否还有 run 在跑：即这个提交点是否真的落在一次 run 中间。
    pub(crate) was_busy: bool,
    /// 重启前是否有调用停在「已登记、未下发」。
    pub(crate) saw_declared: bool,
    /// 重启后的树。副本早于 session 创建时为空。
    pub(crate) tree: Option<SessionTreeSnapshot>,
    /// 重启后链上每个工具调用的结局。
    pub(crate) after: Vec<(String, ToolResultState)>,
    pub(crate) invariants: Vec<InvariantViolation>,
    pub(crate) effects: Vec<Effect>,
}

impl Restart {
    /// 某个调用在重启后的结局。
    pub(crate) fn state_of(&self, call_id: &str) -> Option<&ToolResultState> {
        self.after
            .iter()
            .find(|(id, _)| id == call_id)
            .map(|(_, state)| state)
    }

    /// 某个调用在重启后被重跑的次数。
    pub(crate) fn reruns_of(&self, call_id: &str) -> usize {
        self.effects
            .iter()
            .filter(|effect| effect.call_id == call_id)
            .count()
    }

    /// 某个调用在重启前的结局。
    pub(crate) fn state_before(&self, call_id: &str) -> Option<&ToolResultState> {
        self.before
            .iter()
            .flat_map(|(_, states)| states)
            .find(|(id, _)| id == call_id)
            .map(|(_, state)| state)
    }
}

/// 打开提交点副本，执行启动恢复，跑到静止。
///
/// `Err` 表示这个点自身失败：副本打不开、读不出树，或恢复没有在预期时间内静止。
/// 失败是数据而不是 panic：一个点失败不能打断整次遍历（ADR 0062）。
pub(crate) async fn restart(prefix: &Prefix, session: &SessionId) -> Result<Restart, String> {
    let store = Arc::new(
        Store::open(&prefix.path)
            .await
            .map_err(|error| format!("打开副本失败：{error}"))?,
    );
    let before: Vec<(MessageId, Vec<(String, ToolResultState)>)> =
        match store.transcript(session).await {
            Ok(nodes) => nodes
                .iter()
                .map(|node| {
                    let states: Vec<(String, ToolResultState)> = node
                        .tool_results()
                        .map(|(call, state)| (call.to_owned(), state.clone()))
                        .collect();
                    (node.id, states)
                })
                .collect(),
            Err(durable::StoreError::SessionNotFound(_)) => Vec::new(),
            Err(error) => return Err(format!("读取副本的链失败：{error}")),
        };
    let was_busy = store
        .session_tree(session)
        .await
        .is_ok_and(|tree| tree.session.busy);
    let saw_declared = before
        .iter()
        .flat_map(|(_, states)| states)
        .any(|(_, state)| state == &ToolResultState::Declared);

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
    engine
        .recover()
        .await
        .map_err(|error| format!("启动恢复失败：{error}"))?;
    if !wait_for_rest(&store, session).await {
        return Err(format!("{}：恢复在预期时间内没有静止", prefix.label()));
    }

    let tree = match store.session_tree(session).await {
        Ok(tree) => Some(tree),
        Err(durable::StoreError::SessionNotFound(_)) => None,
        Err(error) => return Err(format!("读取副本的消息树失败：{error}")),
    };
    let after = store
        .transcript(session)
        .await
        .unwrap_or_default()
        .iter()
        .flat_map(|node| {
            node.tool_results()
                .map(|(call, state)| (call.to_owned(), state.clone()))
        })
        .collect();
    Ok(Restart {
        before,
        was_busy,
        saw_declared,
        tree,
        after,
        invariants: store
            .check_invariants()
            .await
            .map_err(|error| format!("检查不变量失败：{error}"))?,
        effects: effects
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone(),
    })
}

/// 轮询直到 session 空闲且队列为空。`false` 表示没有在预期时间内静止。
pub(crate) async fn wait_for_rest(store: &Store, session: &SessionId) -> bool {
    for _ in 0..500 {
        match store.session_tree(session).await {
            Ok(tree) if !tree.session.busy && tree.queue.is_empty() => return true,
            Ok(_) => {}
            Err(durable::StoreError::SessionNotFound(_)) => return true,
            Err(_) => return false,
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    false
}

/// 遍历一个场景的全部崩溃点，返回报告。这是全仓唯一一处遍历。
///
/// 逐点断言在 `support::checks`；这里只收结论、数跨点的事实、追加场景级结论。
pub(crate) async fn walk(scenario: &Scenario, world: &World) -> Report {
    // 逐点重启会打开副本并让恢复写回它们，因此「证据先于执行」必须在遍历之前读。
    let evidence = evidence_findings(world).await;
    let mut mid_run = 0_usize;
    let mut declared_points = 0_usize;
    let expected = usize::try_from(world.commits).unwrap_or(usize::MAX);
    let mut report = Matrix::new(world.prefixes.clone(), expected, checks::PER_POINT)
        .sweep(async |prefix: Prefix| {
            let recovered = match restart(&prefix, &world.session).await {
                Ok(recovered) => recovered,
                Err(detail) => return checks::unavailable(&detail),
            };
            if recovered.was_busy {
                mid_run += 1;
            }
            if recovered.saw_declared {
                declared_points += 1;
            }
            checks::per_point(world, scenario, &prefix, &recovered)
        })
        .await;

    report.extend(evidence);
    report.extend(scenario_findings(scenario, world, mid_run, declared_points));
    report
}

/// 跨点才能得出的结论：矩阵真的落在 run 中间、真的停在「已登记未下发」，以及点表就是
/// 全部提交点。
fn scenario_findings(
    scenario: &Scenario,
    world: &World,
    mid_run: usize,
    declared_points: usize,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    if mid_run == 0 {
        findings.push(Finding::new(
            Kind::Failed,
            "落在 run 中间",
            format!(
                "{}：没有任何提交点落在 run 中间，续跑没有被真正检验",
                scenario.name
            ),
        ));
    }
    if scenario.expect_declared && declared_points == 0 {
        findings.push(Finding::new(
            Kind::Failed,
            "已登记未下发",
            format!(
                "{}：没有任何提交点停在「已登记、未下发」，未下发重跑没有被检验",
                scenario.name
            ),
        ));
    }
    let observed: Vec<u64> = world.prefixes.iter().map(|prefix| prefix.serial).collect();
    let complete: Vec<u64> = (1..=world.commits).collect();
    if observed.len() == complete.len() && observed != complete {
        findings.push(Finding::new(
            Kind::PointMissing,
            "副本序号",
            format!(
                "{}：副本序号 {observed:?} 与已提交次数 {} 不符",
                scenario.name, world.commits
            ),
        ));
    }
    findings
}

/// 每次外部执行都发生在一条已落库的「已下发」之后（P4）。
///
/// 替身执行端记下了每次副作用发生时的已提交次数；打开那一刻的副本，核对那个调用在库里
/// 是「已下发」。这条断言因此量的是库里的行，而不是代码顺序。
async fn evidence_findings(world: &World) -> Vec<Finding> {
    let mut findings = Vec::new();
    for effect in &world.effects {
        let detail = match state_at(world, &effect.call_id, effect.at_commits).await {
            Ok(Some(ToolResultState::Dispatched)) => continue,
            Ok(state) => format!(
                "在提交点 {} 的副本里是 {state:?}，应当是已下发",
                effect.at_commits
            ),
            Err(detail) => detail,
        };
        findings.push(Finding::new(
            Kind::Failed,
            "证据先于执行",
            format!("{}：{detail}", effect.call_id),
        ));
    }
    findings
}

/// 提交点 `serial` 的副本里，某个调用的结局。副本不存在或读不出来都算失败。
async fn state_at(
    world: &World,
    call_id: &str,
    serial: u64,
) -> Result<Option<ToolResultState>, String> {
    let Some(prefix) = world.prefixes.iter().find(|prefix| prefix.serial == serial) else {
        return Err(format!("提交点 {serial} 的副本不存在"));
    };
    let store = Store::open(&prefix.path)
        .await
        .map_err(|error| format!("打开提交点 {serial} 失败：{error}"))?;
    let nodes = store
        .transcript(&world.session)
        .await
        .map_err(|error| format!("读取提交点 {serial} 的链失败：{error}"))?;
    Ok(nodes
        .iter()
        .flat_map(durable::MessageNode::tool_results)
        .find(|(call, _)| *call == call_id)
        .map(|(_, state)| state.clone()))
}

/// 某个节点是否仍在链上。供断言「历史只增不减」。
pub(crate) fn tree_has(tree: &SessionTreeSnapshot, id: MessageId) -> bool {
    let chain = match tree.active_leaf {
        Some(leaf) => MessageTree::new(tree.nodes.clone()).transcript(leaf),
        None => Vec::new(),
    };
    chain.iter().any(|node| node.id == id)
}
