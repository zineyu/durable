//! agent 集成测试的共享启动辅助。
//!
//! 每个集成测试目标是独立 crate，会单独编译本模块，因此这里允许未被该目标使用的条目。

#![allow(dead_code)]

use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use reqwest::{Response, StatusCode};
use sqlx::{ConnectOptions, Row};
use tempfile::TempDir;

pub mod provider;

#[allow(unused_imports)]
pub use provider::{Script, ScriptedProvider, ToolCallSpec};

/// 模型配置的测试取值，用 `Command::envs` 应用到 std 或 tokio 的命令。
pub fn model_environment() -> [(&'static str, &'static str); 3] {
    [
        (agent::MODEL_BASE_URL, "http://127.0.0.1:9/v1"),
        (agent::MODEL_API_KEY, "test-key"),
        (agent::MODEL_NAME, "test-model"),
    ]
}

pub fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_agent"))
}

/// 运行 agent 二进制的命令：指定 SQLite 路径并带上模型配置。
pub fn command(database: &Path) -> std::process::Command {
    let mut command = std::process::Command::new(binary());
    command
        .args(["--database"])
        .arg(database)
        .envs(model_environment());
    command
}

/// 启动 agent 二进制的命令：随机回环端口、捕获 stdout、随进程退出而结束。
pub fn serving_command(database: &Path) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(binary());
    command
        .args(["--listen", "127.0.0.1:0", "--database"])
        .arg(database)
        .envs(model_environment())
        .stdout(Stdio::piped())
        .kill_on_drop(true);
    command
}

/// 读取 agent 打印的监听地址。
pub async fn read_address(child: &mut tokio::process::Child) -> String {
    use tokio::io::AsyncBufReadExt;

    let stdout = child.stdout.take().expect("取得启动输出");
    let mut lines = tokio::io::BufReader::new(stdout).lines();
    let line = tokio::time::timeout(Duration::from_secs(10), lines.next_line())
        .await
        .expect("等待启动输出")
        .expect("读取启动输出")
        .expect("agent 打印监听地址");
    line.strip_prefix("agent listening on http://")
        .expect("启动输出包含监听地址")
        .to_owned()
}

/// 进程内的 agent 实例，监听随机回环端口。
pub struct TestAgent {
    base_url: String,
    handle: tokio::task::JoinHandle<()>,
}

impl TestAgent {
    pub async fn start(
        database: &Path,
        model: &str,
        provider: Arc<dyn agent::ModelProvider>,
        tools: Arc<dyn agent::ToolBox>,
        config: agent::EngineConfig,
    ) -> TestAgent {
        let store = agent::Store::open(database).await.expect("打开 SQLite");
        let listener = tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .await
            .expect("绑定回环端口");
        let address = listener.local_addr().expect("读取本地地址");
        let state = agent::AgentState::start(store, model.to_owned(), provider, tools, config)
            .await
            .expect("启动 agent 状态");
        let handle = tokio::spawn(async move {
            agent::serve(listener, state).await.expect("agent 服务运行");
        });
        TestAgent {
            base_url: format!("http://{address}"),
            handle,
        }
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    pub async fn shutdown(self) {
        self.handle.abort();
        let _ = self.handle.await;
    }
}

/// 临时目录中的 agent、脚本化 provider 与可复用的 HTTP 客户端。
pub struct Harness {
    pub directory: TempDir,
    pub database: PathBuf,
    pub provider: ScriptedProvider,
    pub config: agent::EngineConfig,
    pub limits: agent::ToolLimits,
    agent: TestAgent,
    pub client: reqwest::Client,
}

impl Harness {
    pub async fn start() -> Harness {
        Harness::start_with(agent::EngineConfig::default(), Vec::new()).await
    }

    pub async fn start_with(config: agent::EngineConfig, scripts: Vec<Script>) -> Harness {
        Harness::start_with_limits(config, scripts, agent::ToolLimits::default()).await
    }

    pub async fn start_with_limits(
        config: agent::EngineConfig,
        scripts: Vec<Script>,
        limits: agent::ToolLimits,
    ) -> Harness {
        let directory = tempfile::tempdir().expect("创建临时目录");
        let database = directory.path().join("agent.sqlite");
        let provider = ScriptedProvider::start(scripts).await;
        Harness::open(directory, database, provider, config, limits).await
    }

    async fn open(
        directory: TempDir,
        database: PathBuf,
        provider: ScriptedProvider,
        config: agent::EngineConfig,
        limits: agent::ToolLimits,
    ) -> Harness {
        let model = agent::ModelConfig {
            base_url: provider.base_url(),
            api_key: "test-key".to_owned(),
            model_name: "test-model".to_owned(),
        };
        let runtime = Arc::new(agent::OpenAiProvider::new(&model));
        let tools = Arc::new(agent::ToolRegistry::new(limits));
        let agent = TestAgent::start(&database, "test-model", runtime, tools, config).await;
        Harness {
            directory,
            database,
            provider,
            config,
            limits,
            agent,
            client: reqwest::Client::new(),
        }
    }

    pub async fn restart(self) -> Harness {
        let Harness {
            directory,
            database,
            provider,
            config,
            limits,
            agent,
            ..
        } = self;
        agent.shutdown().await;
        Harness::open(directory, database, provider, config, limits).await
    }

    pub fn url(&self, path: &str) -> String {
        self.agent.url(path)
    }

    pub fn workspace_path(&self, name: &str) -> PathBuf {
        self.directory.path().join(name)
    }

    pub fn workspace(&self, name: &str) -> PathBuf {
        let path = self.workspace_path(name);
        std::fs::create_dir(&path).expect("创建 workspace 目录");
        path
    }

    pub async fn register(&self, path: &str) -> Response {
        self.client
            .post(self.url("/api/workspaces"))
            .json(&serde_json::json!({ "path": path }))
            .send()
            .await
            .expect("注册 workspace")
    }

    pub async fn remove(&self, path: &str) -> Response {
        self.client
            .post(self.url("/api/workspaces/remove"))
            .json(&serde_json::json!({ "path": path }))
            .send()
            .await
            .expect("移除 workspace")
    }

    pub async fn bootstrap(&self) -> agent::BootstrapResponse {
        self.client
            .get(self.url("/api/bootstrap"))
            .send()
            .await
            .expect("读取 bootstrap")
            .json()
            .await
            .expect("解析 bootstrap")
    }

    pub async fn create_session(
        &self,
        workspace: &agent::WorkspacePath,
        title: Option<&str>,
    ) -> Response {
        self.create_session_raw(workspace.as_str(), title).await
    }

    pub async fn create_session_raw(&self, workspace: &str, title: Option<&str>) -> Response {
        self.client
            .post(self.url("/api/sessions"))
            .json(&serde_json::json!({ "workspace": workspace, "title": title }))
            .send()
            .await
            .expect("创建 session")
    }

    pub async fn rename_session(&self, id: &agent::SessionId, title: &str) -> Response {
        self.client
            .post(self.url(&format!("/api/sessions/{}/rename", id.as_str())))
            .json(&serde_json::json!({ "title": title }))
            .send()
            .await
            .expect("重命名 session")
    }

    pub async fn delete_session(&self, id: &agent::SessionId) -> Response {
        self.client
            .post(self.url(&format!("/api/sessions/{}/delete", id.as_str())))
            .send()
            .await
            .expect("删除 session")
    }

    pub async fn session_tree(&self, id: &agent::SessionId) -> Response {
        self.client
            .get(self.url(&format!("/api/sessions/{}/tree", id.as_str())))
            .send()
            .await
            .expect("读取消息树")
    }

    pub async fn tree(&self, id: &agent::SessionId) -> agent::SessionTree {
        self.session_tree(id)
            .await
            .json()
            .await
            .expect("解析消息树")
    }

    pub async fn submit_message(&self, id: &agent::SessionId, text: &str) -> Response {
        self.client
            .post(self.url(&format!("/api/sessions/{}/messages", id.as_str())))
            .json(&serde_json::json!({ "text": text }))
            .send()
            .await
            .expect("提交消息")
    }

    pub async fn cancel_run(&self, id: &agent::SessionId) -> Response {
        self.client
            .post(self.url(&format!("/api/sessions/{}/cancel", id.as_str())))
            .send()
            .await
            .expect("取消 run")
    }

    pub async fn remove_queue_item(
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

    /// 轮询直到该 session 不再忙碌且队列为空。
    pub async fn wait_idle(&self, id: &agent::SessionId) -> agent::SessionTree {
        for _ in 0..500 {
            let tree = self.tree(id).await;
            if !tree.session.busy && tree.queue.is_empty() {
                return tree;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("run 未在预期时间内结束");
    }

    /// 轮询直到该 session 进入忙碌状态。
    pub async fn wait_busy(&self, id: &agent::SessionId) -> agent::SessionTree {
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
    pub async fn wait_for_tree(
        &self,
        id: &agent::SessionId,
        ready: impl Fn(&agent::SessionTree) -> bool,
    ) -> agent::SessionTree {
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
    pub async fn subscribe(&self, after: i64) -> EventStream {
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

    pub async fn registered_workspace(&self, name: &str) -> (PathBuf, agent::WorkspaceMetadata) {
        let workspace = self.workspace(name);
        let path = std::fs::canonicalize(&workspace)
            .expect("规范化路径")
            .to_string_lossy()
            .into_owned();
        let response = self.register(&path).await;
        assert_eq!(response.status(), StatusCode::CREATED);
        let metadata = response.json().await.expect("解析注册响应");
        (workspace, metadata)
    }

    pub async fn created_session(
        &self,
        workspace: &agent::WorkspacePath,
    ) -> agent::SessionMetadata {
        let response = self.create_session(workspace, None).await;
        assert_eq!(response.status(), StatusCode::CREATED);
        response.json().await.expect("解析 session 元数据")
    }

    /// 注册一个新建目录并创建 session，返回 session 元数据。
    pub async fn session(&self, name: &str) -> agent::SessionMetadata {
        let (_, workspace) = self.registered_workspace(name).await;
        self.created_session(&workspace.path).await
    }
}

/// 已订阅的事件流，按 SSE `data:` 行产出协议事件。
pub struct EventStream {
    response: reqwest::Response,
    buffer: Vec<u8>,
}

impl EventStream {
    pub async fn next(&mut self) -> Option<agent::StreamEvent> {
        loop {
            if let Some(position) = self.buffer.iter().position(|byte| *byte == b'\n') {
                let line: Vec<u8> = self.buffer.drain(..=position).collect();
                let line = String::from_utf8(line).expect("事件行是 UTF-8");
                let Some(data) = line.trim_end().strip_prefix("data:") else {
                    continue;
                };
                let data = data.trim();
                if data.is_empty() {
                    continue;
                }
                return Some(serde_json::from_str(data).expect("解析事件"));
            }
            let chunk = self.response.chunk().await.expect("读取事件流")?;
            self.buffer.extend_from_slice(&chunk);
        }
    }
}

pub async fn connect(database: &Path) -> sqlx::SqliteConnection {
    sqlx::sqlite::SqliteConnectOptions::new()
        .filename(database)
        .busy_timeout(Duration::from_secs(5))
        .connect()
        .await
        .expect("打开 SQLite")
}

pub async fn recorded_event_ids(database: &Path) -> Vec<i64> {
    sqlx::query_scalar("SELECT id FROM event ORDER BY id")
        .fetch_all(&mut connect(database).await)
        .await
        .expect("查询事件 id")
}

pub async fn recorded_events(database: &Path) -> Vec<agent::FactEvent> {
    let payloads: Vec<String> = sqlx::query_scalar("SELECT event FROM event ORDER BY id")
        .fetch_all(&mut connect(database).await)
        .await
        .expect("查询事件");
    payloads
        .iter()
        .map(|payload| serde_json::from_str(payload).expect("解析事件"))
        .collect()
}

pub async fn recorded_event_kinds(database: &Path) -> Vec<String> {
    recorded_event_rows(database)
        .await
        .into_iter()
        .map(|(_, _, _, payload)| {
            serde_json::from_str::<serde_json::Value>(&payload).expect("解析事件负载")["kind"]
                .as_str()
                .expect("事件负载有 kind")
                .to_owned()
        })
        .collect()
}

pub async fn recorded_event_rows(database: &Path) -> Vec<(i64, Option<String>, i64, String)> {
    sqlx::query("SELECT id, session_id, created_at, event FROM event ORDER BY id")
        .fetch_all(&mut connect(database).await)
        .await
        .expect("查询事件")
        .iter()
        .map(|row| {
            (
                row.get::<i64, _>(0),
                row.get::<Option<String>, _>(1),
                row.get::<i64, _>(2),
                row.get::<String, _>(3),
            )
        })
        .collect()
}

pub async fn error_code(response: Response) -> agent::ErrorCode {
    error_envelope(response).await.code
}

pub async fn error_envelope(response: Response) -> agent::ErrorResponse {
    response.json().await.expect("解析错误响应")
}
