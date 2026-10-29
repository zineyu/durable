//! 进程级测试脚手架：启动真实 agent 二进制、经 HTTP 驱动它，并在用例结束时杀掉。
//! 进程内的 `Harness` 在父模块，脚本化 provider 在 `provider`。

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use reqwest::StatusCode;

use super::{ScriptedProvider, binary, read_address, serving_command};

/// 与 agent 二进制同目录的 executor 二进制：测试内的传输一律是本地子进程。
pub(crate) fn executor_binary() -> PathBuf {
    let path = binary()
        .parent()
        .expect("agent 二进制有父目录")
        .join("executor");
    assert!(
        path.is_file(),
        "找不到 executor 二进制: {}；先运行 cargo build -p executor，或运行 workspace 级测试。",
        path.display()
    );
    path
}

/// 真实 agent 进程与它的监听地址。
pub(crate) struct Process {
    child: tokio::process::Child,
    base: String,
    database: PathBuf,
}

impl Process {
    /// 启动 agent 二进制，让它指向脚本化 provider 并注册 `workspace`。
    pub(crate) async fn start(
        database: &Path,
        workspace: &Path,
        provider: &ScriptedProvider,
    ) -> Process {
        Process::start_with_environment(database, workspace, provider, &[]).await
    }

    /// 与 [`Process::start`] 相同，但给 agent 进程额外设置环境变量。
    /// ssh 传输的用例据此把假 ssh 放到 `PATH` 首位。
    pub(crate) async fn start_with_environment(
        database: &Path,
        workspace: &Path,
        provider: &ScriptedProvider,
        environment: &[(&str, OsString)],
    ) -> Process {
        let mut command = serving_command(database);
        command
            .arg("--workspace")
            .arg(format!("local:{}", workspace.display()))
            .env(agent::MODEL_BASE_URL, provider.base_url());
        for (key, value) in environment {
            command.env(key, value);
        }
        let mut child = command.spawn().expect("启动 agent");
        let address = read_address(&mut child).await;
        Process {
            child,
            base: format!("http://{address}"),
            database: database.to_path_buf(),
        }
    }

    pub(crate) fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    /// 进程使用的 SQLite 路径：用例直接查库断言已安装的构建 id。
    pub(crate) fn database(&self) -> &Path {
        &self.database
    }

    /// 直接杀掉进程，不经过 HTTP 取消：模拟进程被杀或机器断电。
    pub(crate) async fn kill(mut self) {
        self.child.start_kill().expect("杀掉 agent 进程");
        self.child.wait().await.expect("等待 agent 退出");
    }
}

pub(crate) async fn create_session(process: &Process, workspace: &Path) -> agent::SessionMetadata {
    create_session_on(process, "local", workspace).await
}

/// 在指定 host 上创建 session。workspace 路径必须是已在该 host 上登记的那份。
pub(crate) async fn create_session_on(
    process: &Process,
    host: &str,
    workspace: &Path,
) -> agent::SessionMetadata {
    let path = std::fs::canonicalize(workspace)
        .expect("规范化 workspace 路径")
        .to_string_lossy()
        .into_owned();
    let response = reqwest::Client::new()
        .post(process.url("/api/sessions"))
        .json(&serde_json::json!({ "host": host, "workspace": path, "title": null }))
        .send()
        .await
        .expect("创建 session");
    assert_eq!(
        response.status(),
        StatusCode::CREATED,
        "创建 session 返回 201"
    );
    response.json().await.expect("解析 session 元数据")
}

pub(crate) async fn submit(process: &Process, session: &agent::SessionId, text: &str) {
    let response = reqwest::Client::new()
        .post(process.url(&format!("/api/sessions/{}/messages", session.as_str())))
        .json(&serde_json::json!({ "text": text, "mode": "queue" }))
        .send()
        .await
        .expect("提交消息");
    assert_eq!(response.status(), StatusCode::CREATED, "提交消息返回 201");
}

pub(crate) async fn tree(
    process: &Process,
    session: &agent::SessionId,
) -> agent::SessionTreeSnapshot {
    reqwest::get(process.url(&format!("/api/sessions/{}/tree", session.as_str())))
        .await
        .expect("读取消息树")
        .json()
        .await
        .expect("解析消息树")
}

/// 轮询直到消息树满足条件。
pub(crate) async fn wait_for(
    process: &Process,
    session: &agent::SessionId,
    ready: impl Fn(&agent::SessionTreeSnapshot) -> bool,
) -> agent::SessionTreeSnapshot {
    for _ in 0..500 {
        let tree = tree(process, session).await;
        if ready(&tree) {
            return tree;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("消息树未在预期时间内满足条件");
}

pub(crate) async fn wait_idle(
    process: &Process,
    session: &agent::SessionId,
) -> agent::SessionTreeSnapshot {
    wait_for(process, session, |tree| {
        !tree.session.busy && tree.queue.is_empty()
    })
    .await
}

/// 消息树中指定角色的节点，按 id 升序。
pub(crate) fn nodes_of(
    tree: &agent::SessionTreeSnapshot,
    role: agent::MessageRole,
) -> Vec<&agent::MessageSkeleton> {
    tree.nodes.iter().filter(|node| node.role == role).collect()
}

pub(crate) async fn text_of(
    process: &Process,
    session: &agent::SessionId,
    id: agent::MessageId,
) -> String {
    let response = reqwest::Client::new()
        .post(process.url(&format!(
            "/api/sessions/{}/message-bodies",
            session.as_str()
        )))
        .json(&serde_json::json!({ "messageIds": [id.get()] }))
        .send()
        .await
        .expect("读取消息正文");
    let body: agent::MessageBodiesResponse = response.json().await.expect("解析消息正文");
    body.messages
        .first()
        .map(agent::MessageBody::text)
        .expect("消息有正文")
}
