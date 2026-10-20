//! hub 集成测试的共享启动辅助。
//!
//! 每个集成测试目标是独立 crate，会单独编译本模块，因此这里允许未被该目标使用的条目。

#![allow(dead_code)]
#![allow(unused_imports)]

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use axum::routing::get;
use axum::{Json, Router};
use tempfile::TempDir;

pub use agent_handle::AgentHandle;

/// 进程内的 hub 实例，持有临时数据库并监听随机回环端口。
pub struct Harness {
    pub directory: TempDir,
    pub address: SocketAddr,
}

impl Harness {
    pub async fn start() -> Self {
        Self::start_with(tempfile::tempdir().expect("创建临时目录")).await
    }

    /// 在给定临时目录上启动 hub。复用目录即重启场景。
    pub async fn start_with(directory: TempDir) -> Self {
        let store = durable::Store::open(&database_path(&directory))
            .await
            .expect("打开 hub 数据库");
        let listener = tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .await
            .expect("绑定 hub 端口");
        let address = listener.local_addr().expect("读取 hub 地址");
        tokio::spawn(async move {
            hub::serve(listener, Arc::new(store))
                .await
                .expect("hub 服务运行")
        });
        Self { directory, address }
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.address)
    }

    pub fn database(&self) -> PathBuf {
        database_path(&self.directory)
    }

    pub async fn register_node(&self, name: &str, url: &str) -> reqwest::Response {
        reqwest::Client::new()
            .post(self.url("/api/nodes"))
            .json(&serde_json::json!({ "name": name, "url": url }))
            .send()
            .await
            .expect("请求注册节点")
    }

    pub async fn remove_node(&self, name: &str) -> reqwest::Response {
        reqwest::Client::new()
            .delete(self.url(&format!("/api/nodes/{name}")))
            .send()
            .await
            .expect("请求删除节点")
    }

    pub async fn bootstrap(&self) -> hub::HubBootstrap {
        reqwest::get(self.url("/api/bootstrap"))
            .await
            .expect("请求 bootstrap")
            .json()
            .await
            .expect("解析 bootstrap")
    }

    /// 按名称取出 bootstrap 中的节点快照。
    pub async fn snapshot(&self, name: &str) -> hub::NodeSnapshot {
        self.bootstrap()
            .await
            .nodes
            .into_iter()
            .find(|node| node.name.as_str() == name)
            .unwrap_or_else(|| panic!("bootstrap 缺少节点: {name}"))
    }
}

fn database_path(directory: &TempDir) -> PathBuf {
    directory.path().join("hub.sqlite")
}

mod agent_handle {
    use std::net::Ipv4Addr;
    use std::sync::Arc;

    use tempfile::TempDir;

    /// 进程内的真实 agent：临时 workspace 与 SQLite，模型指向不可用端点。
    pub struct AgentHandle {
        pub directory: TempDir,
        pub url: String,
    }

    impl AgentHandle {
        pub async fn start() -> Self {
            let directory = tempfile::tempdir().expect("创建临时目录");
            let store = agent::Store::open(&directory.path().join("agent.sqlite"))
                .await
                .expect("打开 agent 数据库");
            let model = agent::ModelConfig {
                base_url: "http://127.0.0.1:9/v1".to_owned(),
                api_key: "test-key".to_owned(),
                model_name: "test-model".to_owned(),
            };
            let provider = Arc::new(agent::OpenAiProvider::new(&model));
            let state = agent::AgentState::start(
                store,
                "test-model".to_owned(),
                provider,
                Arc::new(agent::NoTools),
                agent::EngineConfig::default(),
            )
            .await
            .expect("启动 agent 状态");
            let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
                .await
                .expect("绑定 agent 端口");
            let address = listener.local_addr().expect("读取 agent 地址");
            tokio::spawn(async move {
                agent::serve(listener, state).await.expect("agent 服务运行")
            });
            Self {
                directory,
                url: format!("http://{address}"),
            }
        }

        /// 直接向该 agent 注册一个 workspace，用于准备 bootstrap 数据。
        pub async fn register_workspace(&self, path: &str) -> reqwest::Response {
            reqwest::Client::new()
                .post(format!("{}/api/workspaces", self.url))
                .json(&serde_json::json!({ "path": path }))
                .send()
                .await
                .expect("请求注册 workspace")
        }
    }
}

/// 只实现 health 与 bootstrap 的假 agent，用于构造协议版本不匹配的场景。
pub async fn start_fake_agent(protocol_version: u32) -> String {
    let router = Router::new()
        .route(
            "/api/health",
            get(move || async move {
                Json(serde_json::json!({
                    "status": "ok",
                    "version": "0.0.0",
                    "protocolVersion": protocol_version,
                }))
            }),
        )
        .route(
            "/api/bootstrap",
            get(|| async {
                Json(serde_json::json!({
                    "model": "fake",
                    "maxEventId": 0,
                    "workspaces": [],
                    "sessions": [],
                }))
            }),
        );
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("绑定假 agent 端口");
    let address = listener.local_addr().expect("读取假 agent 地址");
    tokio::spawn(async move { axum::serve(listener, router).await.expect("假 agent 运行") });
    format!("http://{address}")
}
