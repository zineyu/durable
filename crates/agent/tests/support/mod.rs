//! agent 集成测试的共享启动辅助。
//!
//! 每个集成测试目标是独立 crate，会单独编译本模块，因此这里允许未被该目标使用的条目。

#![expect(
    dead_code,
    clippy::expect_used,
    clippy::panic,
    clippy::let_underscore_must_use,
    let_underscore_drop,
    unused_imports,
    reason = "测试脚手架用 panic 与 expect 直接报告夹具与断言失败；本模块被每个测试目标各编译一次，未被该目标用到的条目与再导出属正常情况。"
)]

use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use reqwest::{Response, StatusCode};
use tempfile::TempDir;

pub(crate) mod db;
mod event_stream;
mod hosts;
pub(crate) mod ssh;
mod test_agent;

pub(crate) use event_stream::{EventStream, error_code, error_envelope};
pub(crate) use test_agent::TestAgent;
pub(crate) mod process;

pub(crate) use process::{kill_process, log_directory, read_address, serving_command};
pub(crate) mod provider;

pub(crate) use db::{
    RecordedFact, connect, corrupt_message_parts, installed_build, recorded_event_ids,
    recorded_event_kinds, recorded_event_rows, recorded_events, recorded_facts,
    reject_compaction_writes,
};

pub(crate) use provider::{Script, ScriptedProvider};
pub(crate) use ssh::FakeSsh;

/// 模型配置的测试取值，用 `Command::envs` 应用到 std 或 tokio 的命令。
pub(crate) fn model_environment() -> [(&'static str, &'static str); 3] {
    [
        (agent::MODEL_BASE_URL, "http://127.0.0.1:9/v1"),
        (agent::MODEL_API_KEY, "test-key"),
        (agent::MODEL_NAME, "test-model"),
    ]
}

pub(crate) fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_agent"))
}

/// 运行 agent 二进制的命令：指定 SQLite 与日志路径并带上模型配置。
pub(crate) fn command(database: &Path) -> std::process::Command {
    let mut command = command_without_paths();
    command
        .args(["--database"])
        .arg(database)
        .args(["--log-dir"])
        .arg(log_directory(database));
    command
}

/// 未指定数据库与日志路径的 agent 命令：用于验证标准目录，也供 [`command`] 派生。
pub(crate) fn command_without_paths() -> std::process::Command {
    let mut command = std::process::Command::new(binary());
    command
        .envs(model_environment())
        .env(runtime::logging::FILTER_ENV, "warn");
    command
}

/// 临时目录中的 agent、脚本化 provider 与可复用的 HTTP 客户端。
pub(crate) struct Harness {
    pub directory: TempDir,
    pub database: PathBuf,
    pub provider: ScriptedProvider,
    pub config: agent::EngineConfig,
    pub limits: agent::ToolLimits,
    executor: PathBuf,
    heartbeat: Duration,
    executor_pid: Option<u32>,
    agent: TestAgent,
    pub client: reqwest::Client,
}

impl Harness {
    pub(crate) async fn start() -> Harness {
        Harness::start_with(agent::EngineConfig::default(), Vec::new()).await
    }

    pub(crate) async fn start_with(config: agent::EngineConfig, scripts: Vec<Script>) -> Harness {
        Harness::start_with_limits(config, scripts, agent::ToolLimits::default()).await
    }

    pub(crate) async fn start_with_limits(
        config: agent::EngineConfig,
        scripts: Vec<Script>,
        limits: agent::ToolLimits,
    ) -> Harness {
        Harness::start_configured(options::Options {
            config,
            scripts,
            limits,
            ..options::Options::default()
        })
        .await
    }

    /// 用给定的 executor 路径与心跳间隔启动：host 生命周期用例据此注入替身与快心跳。
    pub(crate) async fn start_with_executor(executor: PathBuf, heartbeat: Duration) -> Harness {
        Harness::start_configured(options::Options {
            executor,
            heartbeat,
            ..options::Options::default()
        })
        .await
    }

    async fn start_configured(options: options::Options) -> Harness {
        let directory = tempfile::tempdir().expect("创建临时目录");
        let database = directory.path().join("agent.sqlite");
        let provider = ScriptedProvider::start(options.scripts).await;
        Harness::open(
            directory,
            database,
            provider,
            options.config,
            options.limits,
            options.executor,
            options.heartbeat,
        )
        .await
    }

    async fn open(
        directory: TempDir,
        database: PathBuf,
        provider: ScriptedProvider,
        config: agent::EngineConfig,
        limits: agent::ToolLimits,
        executor: PathBuf,
        heartbeat: Duration,
    ) -> Harness {
        let model = agent::ModelConfig {
            base_url: provider.base_url(),
            api_key: "test-key".to_owned(),
            model_name: "test-model".to_owned(),
        };
        let runtime = Arc::new(agent::OpenAiProvider::new(&model));
        let (agent, hosts) = TestAgent::start(
            &database,
            &executor,
            limits,
            heartbeat,
            "test-model",
            runtime,
            config,
        )
        .await;
        let executor_pid = await_local_executor_pid(&hosts).await;
        Harness {
            directory,
            database,
            provider,
            config,
            limits,
            executor,
            heartbeat,
            executor_pid,
            agent,
            client: reqwest::Client::new(),
        }
    }

    pub(crate) async fn restart(self) -> Harness {
        let Harness {
            directory,
            database,
            provider,
            config,
            limits,
            executor,
            heartbeat,
            agent,
            ..
        } = self;
        agent.shutdown().await;
        Harness::open(
            directory, database, provider, config, limits, executor, heartbeat,
        )
        .await
    }

    pub(crate) fn url(&self, path: &str) -> String {
        self.agent.url(path)
    }

    /// 本 harness 的 executor 子进程 pid：断链用例直接杀掉它。
    pub(crate) fn executor_pid(&self) -> u32 {
        self.executor_pid
            .expect("本 harness 的 local host 启动过 executor 子进程")
    }

    pub(crate) fn workspace_path(&self, name: &str) -> PathBuf {
        self.directory.path().join(name)
    }

    pub(crate) fn workspace(&self, name: &str) -> PathBuf {
        let path = self.workspace_path(name);
        std::fs::create_dir(&path).expect("创建 workspace 目录");
        path
    }

    pub(crate) async fn bootstrap(&self) -> agent::BootstrapResponse {
        self.client
            .get(self.url("/api/bootstrap"))
            .send()
            .await
            .expect("读取 bootstrap")
            .json()
            .await
            .expect("解析 bootstrap")
    }

    pub(crate) async fn create_session(
        &self,
        workspace: &agent::WorkspacePath,
        title: Option<&str>,
    ) -> Response {
        self.create_session_raw(workspace.as_str(), title).await
    }

    pub(crate) async fn create_session_raw(
        &self,
        workspace: &str,
        title: Option<&str>,
    ) -> Response {
        self.create_session_on("local", workspace, title).await
    }

    /// 在指定 host 上创建 session。
    pub(crate) async fn create_session_on(
        &self,
        host: &str,
        workspace: &str,
        title: Option<&str>,
    ) -> Response {
        self.client
            .post(self.url("/api/sessions"))
            .json(&serde_json::json!({ "host": host, "workspace": workspace, "title": title }))
            .send()
            .await
            .expect("创建 session")
    }

    pub(crate) async fn rename_session(&self, id: &agent::SessionId, title: &str) -> Response {
        self.client
            .post(self.url(&format!("/api/sessions/{}/rename", id.as_str())))
            .json(&serde_json::json!({ "title": title }))
            .send()
            .await
            .expect("重命名 session")
    }

    pub(crate) async fn delete_session(&self, id: &agent::SessionId) -> Response {
        self.client
            .post(self.url(&format!("/api/sessions/{}/delete", id.as_str())))
            .send()
            .await
            .expect("删除 session")
    }

    pub(crate) async fn session_tree(&self, id: &agent::SessionId) -> Response {
        self.client
            .get(self.url(&format!("/api/sessions/{}/tree", id.as_str())))
            .send()
            .await
            .expect("读取消息树")
    }

    pub(crate) async fn tree(&self, id: &agent::SessionId) -> agent::SessionTreeSnapshot {
        let response = self.session_tree(id).await;
        let status = response.status();
        let text = response.text().await.expect("读取消息树响应");
        assert_eq!(status, StatusCode::OK, "读取消息树失败: {text}");
        serde_json::from_str(&text).expect("解析消息树")
    }

    /// 以排队语义提交消息。
    pub(crate) async fn submit_message(&self, id: &agent::SessionId, text: &str) -> Response {
        self.submit_mode_message(id, text, "queue").await
    }

    /// 以插话语义提交消息。
    pub(crate) async fn submit_steer_message(&self, id: &agent::SessionId, text: &str) -> Response {
        self.submit_mode_message(id, text, "steer").await
    }

    async fn submit_mode_message(&self, id: &agent::SessionId, text: &str, mode: &str) -> Response {
        self.client
            .post(self.url(&format!("/api/sessions/{}/messages", id.as_str())))
            .json(&serde_json::json!({ "text": text, "mode": mode }))
            .send()
            .await
            .expect("提交消息")
    }

    pub(crate) async fn edit_message(
        &self,
        id: &agent::SessionId,
        message: agent::MessageId,
        text: &str,
    ) -> Response {
        self.client
            .post(self.url(&format!(
                "/api/sessions/{}/messages/{}/edit",
                id.as_str(),
                message.get()
            )))
            .json(&serde_json::json!({ "text": text }))
            .send()
            .await
            .expect("编辑用户消息")
    }

    pub(crate) async fn retry_message(
        &self,
        id: &agent::SessionId,
        message: agent::MessageId,
    ) -> Response {
        self.client
            .post(self.url(&format!(
                "/api/sessions/{}/messages/{}/retry",
                id.as_str(),
                message.get()
            )))
            .send()
            .await
            .expect("重试 assistant 消息")
    }

    pub(crate) async fn switch_active_leaf(
        &self,
        id: &agent::SessionId,
        message: agent::MessageId,
    ) -> Response {
        self.client
            .post(self.url(&format!("/api/sessions/{}/active-leaf", id.as_str())))
            .json(&serde_json::json!({ "messageId": message.get() }))
            .send()
            .await
            .expect("切换 active leaf")
    }

    pub(crate) async fn cancel_run(&self, id: &agent::SessionId) -> Response {
        self.client
            .post(self.url(&format!("/api/sessions/{}/cancel", id.as_str())))
            .send()
            .await
            .expect("取消 run")
    }

    pub(crate) async fn remove_queue_item(
        &self,
        id: &agent::SessionId,
        item: agent::QueueItemId,
    ) -> Response {
        self.client
            .post(self.url(&format!(
                "/api/sessions/{}/queue/{}/remove",
                id.as_str(),
                item.get()
            )))
            .send()
            .await
            .expect("删除队列项")
    }

    /// 按消息 id 批量读取块正文。
    pub(crate) async fn bodies(
        &self,
        id: &agent::SessionId,
        messages: &[agent::MessageId],
    ) -> Vec<agent::MessageBody> {
        let response = self
            .client
            .post(self.url(&format!("/api/sessions/{}/message-bodies", id.as_str())))
            .json(&serde_json::json!({
                "messageIds": messages.iter().map(|id| id.get()).collect::<Vec<_>>(),
            }))
            .send()
            .await
            .expect("读取消息正文");
        assert_eq!(response.status(), StatusCode::OK, "读取正文返回 200");
        let body: agent::MessageBodiesResponse = response.json().await.expect("解析消息正文");
        body.messages
    }

    /// 读取单条消息的块正文。
    pub(crate) async fn body(
        &self,
        id: &agent::SessionId,
        message: agent::MessageId,
    ) -> agent::MessageBody {
        self.bodies(id, &[message])
            .await
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("消息 {} 没有正文", message.get()))
    }

    /// 读取单条消息的文本块拼接结果。
    pub(crate) async fn text(&self, id: &agent::SessionId, message: agent::MessageId) -> String {
        self.body(id, message).await.text()
    }

    /// 轮询直到该 session 不再忙碌且队列为空。
    pub(crate) async fn wait_idle(&self, id: &agent::SessionId) -> agent::SessionTreeSnapshot {
        for _ in 0..500 {
            let tree = self.tree(id).await;
            if !tree.session.busy && tree.queue.is_empty() {
                return tree;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("run 未在预期时间内结束");
    }

    /// 等待脚本化 provider 收到至少 `count` 次请求。
    ///
    /// 脚本按顺序发放。在请求到达之前取消 run 或重启进程，会把脚本留给下一个 run，
    /// 使后续断言落空。取消挂起的 run 或重启之前用它确认脚本已被取走。
    pub(crate) async fn wait_for_requests(&self, count: usize) {
        for _ in 0..500 {
            if self.provider.requests().len() >= count {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("provider 未在预期时间内收到 {count} 次请求");
    }

    /// 轮询直到该 session 进入忙碌状态。
    pub(crate) async fn wait_busy(&self, id: &agent::SessionId) -> agent::SessionTreeSnapshot {
        for _ in 0..500 {
            let tree = self.tree(id).await;
            if tree.session.busy {
                return tree;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("run 未在预期时间内开始");
    }

    /// 轮询直到消息树满足条件。
    pub(crate) async fn wait_for_tree(
        &self,
        id: &agent::SessionId,
        ready: impl Fn(&agent::SessionTreeSnapshot) -> bool,
    ) -> agent::SessionTreeSnapshot {
        for _ in 0..500 {
            let tree = self.tree(id).await;
            if ready(&tree) {
                return tree;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("消息树未在预期时间内满足条件");
    }

    /// 订阅全局事件流。`after` 为 0 时回放全部历史事件。
    pub(crate) async fn subscribe(&self, after: i64) -> EventStream {
        let response = self
            .client
            .get(self.url(&format!("/api/events?after={after}")))
            .send()
            .await
            .expect("订阅事件流");
        EventStream {
            response,
            buffer: Vec::new(),
        }
    }

    pub(crate) async fn registered_workspace(
        &self,
        name: &str,
    ) -> (PathBuf, agent::WorkspaceMetadata) {
        let workspace = self.workspace(name);
        let path = std::fs::canonicalize(&workspace)
            .expect("规范化路径")
            .to_string_lossy()
            .into_owned();
        let response = self.register(&path).await;
        assert_eq!(
            response.status(),
            StatusCode::CREATED,
            "注册 workspace 返回 201"
        );
        let metadata = response.json().await.expect("解析注册响应");
        (workspace, metadata)
    }

    pub(crate) async fn created_session(
        &self,
        workspace: &agent::WorkspacePath,
    ) -> agent::SessionMetadata {
        let response = self.create_session(workspace, None).await;
        assert_eq!(
            response.status(),
            StatusCode::CREATED,
            "创建 session 返回 201"
        );
        response.json().await.expect("解析 session 元数据")
    }

    /// 注册一个新建目录并创建 session，返回 session 元数据。
    pub(crate) async fn session(&self, name: &str) -> agent::SessionMetadata {
        let (_, workspace) = self.registered_workspace(name).await;
        self.created_session(&workspace.path).await
    }
}

/// 本机 host 的 executor 子进程 pid。link 由后台任务建立，先等待它到 `ok`。
async fn await_local_executor_pid(hosts: &agent::HostRegistry) -> Option<u32> {
    let local = agent::HostName::parse(agent::LOCAL_HOST).expect("内置 host 名合法");
    hosts.await_ready(&local, agent::STARTUP_READY_WAIT).await;
    hosts.link(&local).await.and_then(|link| link.pid())
}

/// harness 的启动参数。字段都有默认值，用例只覆盖自己关心的那几项。
mod options {
    use std::path::PathBuf;
    use std::time::Duration;

    use crate::support::{Script, process};

    pub(super) struct Options {
        pub config: agent::EngineConfig,
        pub scripts: Vec<Script>,
        pub limits: agent::ToolLimits,
        pub executor: PathBuf,
        pub heartbeat: Duration,
    }

    impl Default for Options {
        fn default() -> Self {
            Self {
                config: agent::EngineConfig::default(),
                scripts: Vec::new(),
                limits: agent::ToolLimits::default(),
                executor: process::executor_binary(),
                heartbeat: agent::HEARTBEAT_INTERVAL,
            }
        }
    }
}
