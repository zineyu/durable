//! host 与 workspace 的 HTTP 命令：注册、更新、删除、重连与快照轮询。
//! 命令行助手在 `process`，一次性请求的公共部分在 `mod`。

use super::*;

impl Harness {
    /// 在 `local` host 上注册 workspace。
    pub(crate) async fn register(&self, path: &str) -> Response {
        self.register_on("local", path).await
    }

    /// 在指定 host 上注册 workspace。
    pub(crate) async fn register_on(&self, host: &str, path: &str) -> Response {
        self.client
            .post(self.url("/api/workspaces"))
            .json(&serde_json::json!({ "host": host, "path": path }))
            .send()
            .await
            .expect("注册 workspace")
    }

    /// 从 `local` host 移除 workspace。
    pub(crate) async fn remove(&self, path: &str) -> Response {
        self.remove_on("local", path).await
    }

    /// 从指定 host 移除 workspace。
    pub(crate) async fn remove_on(&self, host: &str, path: &str) -> Response {
        self.client
            .post(self.url("/api/workspaces/remove"))
            .json(&serde_json::json!({ "host": host, "path": path }))
            .send()
            .await
            .expect("移除 workspace")
    }

    /// 注册 host。
    pub(crate) async fn register_host(
        &self,
        name: &str,
        kind: &str,
        target: Option<&str>,
    ) -> Response {
        self.client
            .post(self.url("/api/hosts"))
            .json(&serde_json::json!({ "name": name, "kind": kind, "target": target }))
            .send()
            .await
            .expect("注册 host")
    }

    /// 局部更新 host。
    pub(crate) async fn update_host(&self, name: &str, body: serde_json::Value) -> Response {
        self.client
            .patch(self.url(&format!("/api/hosts/{name}")))
            .json(&body)
            .send()
            .await
            .expect("更新 host")
    }

    /// 删除 host。
    pub(crate) async fn remove_host(&self, name: &str) -> Response {
        self.client
            .delete(self.url(&format!("/api/hosts/{name}")))
            .send()
            .await
            .expect("删除 host")
    }

    /// 手动重连 host。
    pub(crate) async fn reconnect_host(&self, name: &str) -> Response {
        self.client
            .post(self.url(&format!("/api/hosts/{name}/reconnect")))
            .send()
            .await
            .expect("重连 host")
    }

    /// 轮询直到 host 快照满足条件。
    pub(crate) async fn host_snapshot(
        &self,
        name: &str,
        ready: impl Fn(&agent::HostSnapshot) -> bool,
    ) -> agent::HostSnapshot {
        for _ in 0..500 {
            let bootstrap = self.bootstrap().await;
            if let Some(snapshot) = bootstrap
                .hosts
                .iter()
                .find(|host| host.name.as_str() == name && ready(host))
            {
                return snapshot.clone();
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("host {name} 未在预期时间内满足条件");
    }
}
