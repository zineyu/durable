//! 进程内 hub 与 agent 的启动辅助：临时数据库、随机回环端口、重启与轮询等待。

#![expect(
    dead_code,
    clippy::expect_used,
    clippy::panic,
    clippy::let_underscore_must_use,
    let_underscore_drop,
    reason = "测试脚手架用 expect 与 panic 直接报告夹具与断言失败：夹具失败必须立刻暴露。\
              本模块被每个集成测试目标独立编译，条目未必都被使用。"
)]

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tempfile::TempDir;

/// 进程内的 hub 实例，持有临时数据库并监听随机回环端口。
pub(crate) struct Harness {
    pub directory: TempDir,
    pub address: SocketAddr,
    task: tokio::task::JoinHandle<()>,
}

impl Harness {
    pub(crate) async fn start() -> Self {
        Self::start_with(tempfile::tempdir().expect("创建临时目录")).await
    }

    /// 用较小的总线容量启动，用于覆盖落后与窗口外游标。
    pub(crate) async fn start_with_options(options: hub::HubOptions) -> Self {
        Self::start_options(tempfile::tempdir().expect("创建临时目录"), options).await
    }

    /// 在给定临时目录上启动 hub。复用目录即重启场景。
    pub(crate) async fn start_with(directory: TempDir) -> Self {
        Self::start_options(directory, hub::HubOptions::default()).await
    }

    pub(crate) async fn start_options(directory: TempDir, options: hub::HubOptions) -> Self {
        let database = database_path(&directory);
        let listener = tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .await
            .expect("绑定 hub 端口");
        let address = listener.local_addr().expect("读取 hub 地址");
        let task = tokio::spawn(async move {
            hub::serve_with(listener, &database, options)
                .await
                .expect("hub 服务运行");
        });
        Self {
            directory,
            address,
            task,
        }
    }

    /// 结束本实例并取回临时目录，用于在同一数据库上重启 hub。
    ///
    /// 必须等任务真正退出：否则旧实例仍持有 SQLite 连接与监听端口，重启会与之竞争。
    pub(crate) async fn stop(self) -> TempDir {
        self.task.abort();
        let _ = self.task.await;
        self.directory
    }

    pub(crate) fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.address)
    }

    pub(crate) fn database(&self) -> PathBuf {
        database_path(&self.directory)
    }

    pub(crate) async fn register_node(&self, name: &str, url: &str) -> reqwest::Response {
        reqwest::Client::new()
            .post(self.url("/api/nodes"))
            .json(&serde_json::json!({ "name": name, "url": url }))
            .send()
            .await
            .expect("请求注册节点")
    }

    pub(crate) async fn update_node(
        &self,
        name: &str,
        body: serde_json::Value,
    ) -> reqwest::Response {
        reqwest::Client::new()
            .patch(self.url(&format!("/api/nodes/{name}")))
            .json(&body)
            .send()
            .await
            .expect("请求修改节点")
    }

    pub(crate) async fn rename_node(&self, name: &str, next: &str) -> reqwest::Response {
        self.update_node(name, serde_json::json!({ "name": next }))
            .await
    }

    pub(crate) async fn set_node_enabled(&self, name: &str, enabled: bool) -> reqwest::Response {
        self.update_node(name, serde_json::json!({ "enabled": enabled }))
            .await
    }

    pub(crate) async fn reconnect_node(&self, name: &str) -> reqwest::Response {
        reqwest::Client::new()
            .post(self.url(&format!("/api/nodes/{name}/reconnect")))
            .send()
            .await
            .expect("请求重连节点")
    }

    pub(crate) async fn remove_node(&self, name: &str) -> reqwest::Response {
        reqwest::Client::new()
            .delete(self.url(&format!("/api/nodes/{name}")))
            .send()
            .await
            .expect("请求删除节点")
    }

    pub(crate) async fn bootstrap(&self) -> hub::HubBootstrap {
        reqwest::get(self.url("/api/bootstrap"))
            .await
            .expect("请求 bootstrap")
            .json()
            .await
            .expect("解析 bootstrap")
    }

    /// 轮询等待某个节点到达指定状态。上游状态由总线异步更新。
    pub(crate) async fn wait_for_status(
        &self,
        name: &str,
        status: hub::NodeStatus,
    ) -> hub::NodeSnapshot {
        for _ in 0..500 {
            let snapshot = self.snapshot(name).await;
            if snapshot.status == status {
                return snapshot;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("节点 {name} 未在超时内到达状态 {status:?}");
    }

    /// 等待广播序号推进到 `target` 之后。
    pub(crate) async fn wait_for_seq_after(&self, target: u64) -> u64 {
        for _ in 0..500 {
            let seq = self.bootstrap().await.max_hub_seq;
            if seq > target {
                return seq;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("广播序号未在超时内超过 {target}");
    }

    /// 按名称取出 bootstrap 中的节点快照。
    pub(crate) async fn snapshot(&self, name: &str) -> hub::NodeSnapshot {
        self.bootstrap()
            .await
            .nodes
            .into_iter()
            .find(|node| node.name.as_str() == name)
            .unwrap_or_else(|| panic!("bootstrap 缺少节点: {name}"))
    }
}

/// 与 hub 测试二进制同目录的 executor 二进制：本机 host 的传输是本地子进程。
fn executor_binary() -> PathBuf {
    std::env::current_exe()
        .expect("测试二进制有路径")
        .parent()
        .and_then(|deps| deps.parent())
        .expect("测试二进制有父目录")
        .join("executor")
}

fn database_path(directory: &TempDir) -> PathBuf {
    directory.path().join("hub.sqlite")
}

/// 进程内的真实 agent：临时 workspace 与 SQLite，模型指向不可用端点。
pub(crate) struct AgentHandle {
    pub directory: TempDir,
    pub url: String,
}

impl AgentHandle {
    pub(crate) async fn start() -> Self {
        let directory = tempfile::tempdir().expect("创建临时目录");
        let store = Arc::new(
            agent::Store::open(&directory.path().join("agent.sqlite"))
                .await
                .expect("打开 agent 数据库"),
        );
        let hosts = agent::HostRegistry::start(
            Arc::clone(&store),
            executor_binary(),
            agent::ToolLimits::default(),
        )
        .await
        .expect("启动 host registry");
        let model = agent::ModelConfig {
            base_url: "http://127.0.0.1:9/v1".to_owned(),
            api_key: "test-key".to_owned(),
            model_name: "test-model".to_owned(),
        };
        let provider = Arc::new(agent::OpenAiProvider::new(&model));
        let tools = Arc::new(agent::RemoteTools::new(Arc::clone(&hosts)));
        let state = agent::AgentState::start(
            store,
            hosts,
            "test-model".to_owned(),
            provider,
            tools,
            agent::EngineConfig::default(),
        )
        .await
        .expect("启动 agent 状态");
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("绑定 agent 端口");
        let address = listener.local_addr().expect("读取 agent 地址");
        tokio::spawn(async move {
            agent::serve(listener, state).await.expect("agent 服务运行");
        });
        Self {
            directory,
            url: format!("http://{address}"),
        }
    }

    /// 直接向该 agent 注册一个 workspace，用于准备 bootstrap 数据。
    pub(crate) async fn register_workspace(&self, path: &str) -> reqwest::Response {
        reqwest::Client::new()
            .post(format!("{}/api/workspaces", self.url))
            .json(&serde_json::json!({ "host": "local", "path": path }))
            .send()
            .await
            .expect("请求注册 workspace")
    }
}
