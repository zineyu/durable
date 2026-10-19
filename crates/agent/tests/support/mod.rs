//! agent 集成测试的共享启动辅助。
//!
//! 每个集成测试目标是独立 crate，会单独编译本模块，因此这里允许未被该目标使用的条目。

#![allow(dead_code)]

use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

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
    pub async fn start(database: &Path, model: &str) -> TestAgent {
        let store = agent::Store::open(database).expect("打开 SQLite");
        let listener = tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .await
            .expect("绑定回环端口");
        let address = listener.local_addr().expect("读取本地地址");
        let model = model.to_owned();
        let handle = tokio::spawn(async move {
            agent::serve(listener, agent::AgentState::new(store, model))
                .await
                .expect("agent 服务运行");
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
