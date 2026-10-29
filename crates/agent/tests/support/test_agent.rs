//! 进程内的 agent 实例：临时目录、临时 SQLite 与真实 HTTP 服务。
//! 子进程形态在 `process`，host 命令在 `hosts`。

use super::*;

/// 进程内的 agent 实例，监听随机回环端口。
pub(crate) struct TestAgent {
    base_url: String,
    handle: tokio::task::JoinHandle<()>,
}

impl TestAgent {
    pub(crate) async fn start(
        database: &Path,
        executor: &Path,
        limits: agent::ToolLimits,
        heartbeat: Duration,
        model: &str,
        provider: Arc<dyn agent::ModelProvider>,
        config: agent::EngineConfig,
    ) -> (TestAgent, Arc<agent::HostRegistry>) {
        let store = Arc::new(agent::Store::open(database).await.expect("打开 SQLite"));
        let hosts = agent::HostRegistry::start_with_interval(
            Arc::clone(&store),
            executor.to_path_buf(),
            limits,
            heartbeat,
        )
        .await
        .expect("启动 host registry");
        let tools = Arc::new(agent::RemoteTools::new(Arc::clone(&hosts)));
        let listener = tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .await
            .expect("绑定回环端口");
        let address = listener.local_addr().expect("读取本地地址");
        let state = agent::AgentState::start(
            store,
            Arc::clone(&hosts),
            model.to_owned(),
            provider,
            tools,
            config,
        )
        .await
        .expect("启动 agent 状态");
        let handle = tokio::spawn(async move {
            agent::serve(listener, state).await.expect("agent 服务运行");
        });
        (
            TestAgent {
                base_url: format!("http://{address}"),
                handle,
            },
            hosts,
        )
    }

    pub(crate) fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    pub(crate) async fn shutdown(self) {
        self.handle.abort();
        let _ = self.handle.await;
    }
}
