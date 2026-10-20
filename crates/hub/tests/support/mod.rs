//! hub 集成测试的共享启动辅助。
//!
//! 每个集成测试目标是独立 crate，会单独编译本模块，因此这里允许未被该目标使用的条目。

#![allow(dead_code)]

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use tempfile::TempDir;

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
}

fn database_path(directory: &TempDir) -> PathBuf {
    directory.path().join("hub.sqlite")
}
