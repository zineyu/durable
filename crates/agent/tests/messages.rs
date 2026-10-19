use std::path::{Path, PathBuf};
use std::time::Duration;

use reqwest::{Response, StatusCode};
use sqlx::ConnectOptions;
use tempfile::TempDir;

mod support;

struct Harness {
    directory: TempDir,
    database: PathBuf,
    agent: support::TestAgent,
    client: reqwest::Client,
}

impl Harness {
    async fn start() -> Harness {
        let directory = tempfile::tempdir().expect("创建临时目录");
        let database = directory.path().join("agent.sqlite");
        Harness::open(directory, database).await
    }

    async fn open(directory: TempDir, database: PathBuf) -> Harness {
        let agent = support::TestAgent::start(&database, "test-model").await;
        Harness {
            directory,
            database,
            agent,
            client: reqwest::Client::new(),
        }
    }

    async fn restart(self) -> Harness {
        let Harness {
            directory,
            database,
            agent,
            ..
        } = self;
        agent.shutdown().await;
        Harness::open(directory, database).await
    }

    fn url(&self, path: &str) -> String {
        self.agent.url(path)
    }

    async fn session(&self, name: &str) -> agent::SessionMetadata {
        let directory = self.directory.path().join(name);
        std::fs::create_dir(&directory).expect("创建 workspace 目录");
        let path = std::fs::canonicalize(&directory)
            .expect("规范化路径")
            .to_string_lossy()
            .into_owned();
        let response = self
            .client
            .post(self.url("/api/workspaces"))
            .json(&serde_json::json!({ "path": path }))
            .send()
            .await
            .expect("注册 workspace");
        assert_eq!(response.status(), StatusCode::CREATED);
        let response = self
            .client
            .post(self.url("/api/sessions"))
            .json(&serde_json::json!({ "workspace": path }))
            .send()
            .await
            .expect("创建 session");
        assert_eq!(response.status(), StatusCode::CREATED);
        response.json().await.expect("解析 session 元数据")
    }

    async fn submit(&self, id: &agent::SessionId, text: &str) -> Response {
        self.client
            .post(self.url(&format!("/api/sessions/{}/messages", id.as_str())))
            .json(&serde_json::json!({ "text": text }))
            .send()
            .await
            .expect("提交消息")
    }

    async fn tree(&self, id: &agent::SessionId) -> agent::SessionTree {
        self.client
            .get(self.url(&format!("/api/sessions/{}/tree", id.as_str())))
            .send()
            .await
            .expect("读取消息树")
            .json()
            .await
            .expect("解析消息树")
    }
}

async fn connect(database: &Path) -> sqlx::SqliteConnection {
    sqlx::sqlite::SqliteConnectOptions::new()
        .filename(database)
        .busy_timeout(Duration::from_secs(5))
        .connect()
        .await
        .expect("打开 SQLite")
}

async fn error_code(response: Response) -> agent::ErrorCode {
    let envelope: agent::ErrorResponse = response.json().await.expect("解析错误响应");
    envelope.code
}

/// 规格「事件信封」与「消息节点」：第一条消息成为树的根，active leaf 随之移动。
#[tokio::test]
async fn appends_first_message_as_tree_root() {
    let harness = Harness::start().await;
    let session = harness.session("workspace").await;

    let response = harness.submit(&session.id, "检查仓库状态").await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let node: agent::MessageNode = response.json().await.expect("解析消息节点");
    assert_eq!(node.parent_id, None);
    assert_eq!(node.role, agent::MessageRole::User);
    assert_eq!(node.text.as_str(), "检查仓库状态");
    assert!(node.created_at > 0);

    let tree = harness.tree(&session.id).await;
    assert_eq!(tree.active_leaf, Some(node.id));
    assert_eq!(tree.nodes, vec![node.clone()]);
    assert_eq!(tree.session.updated_at, node.created_at);
}

/// 规格「消息树」：后续消息以当前 active leaf 为父节点，id 全局递增。
#[tokio::test]
async fn appends_messages_along_the_active_leaf() {
    let harness = Harness::start().await;
    let session = harness.session("workspace").await;

    let first: agent::MessageNode = harness
        .submit(&session.id, "第一行")
        .await
        .json()
        .await
        .expect("解析第一条消息");
    let second: agent::MessageNode = harness
        .submit(&session.id, "第二行\n第三行")
        .await
        .json()
        .await
        .expect("解析第二条消息");

    assert_eq!(second.parent_id, Some(first.id));
    assert!(second.id > first.id, "消息 id 全局递增");
    assert_eq!(second.text.as_str(), "第二行\n第三行");

    let tree = harness.tree(&session.id).await;
    assert_eq!(tree.active_leaf, Some(second.id));
    assert_eq!(tree.nodes, vec![first, second]);
}

/// 不同 session 共享一个消息 id 分配器。
#[tokio::test]
async fn allocates_message_ids_across_sessions() {
    let harness = Harness::start().await;
    let first_session = harness.session("first").await;
    let second_session = harness.session("second").await;

    let first: agent::MessageNode = harness
        .submit(&first_session.id, "第一个 session")
        .await
        .json()
        .await
        .expect("解析第一条消息");
    let second: agent::MessageNode = harness
        .submit(&second_session.id, "第二个 session")
        .await
        .json()
        .await
        .expect("解析第二条消息");

    assert_eq!(second.parent_id, None, "另一个 session 从根开始");
    assert!(second.id > first.id, "消息 id 全局递增");
}

/// 规格「持久化」：消息树在 agent 重启后保持，id 分配器继续递增。
#[tokio::test]
async fn restores_messages_after_restart() {
    let harness = Harness::start().await;
    let session = harness.session("workspace").await;
    let node: agent::MessageNode = harness
        .submit(&session.id, "重启前的消息")
        .await
        .json()
        .await
        .expect("解析消息节点");

    let harness = harness.restart().await;
    let tree = harness.tree(&session.id).await;
    assert_eq!(tree.active_leaf, Some(node.id));
    assert_eq!(tree.nodes, vec![node.clone()]);

    let next: agent::MessageNode = harness
        .submit(&session.id, "重启后的消息")
        .await
        .json()
        .await
        .expect("解析新消息");
    assert_eq!(next.parent_id, Some(node.id));
    assert!(next.id > node.id, "分配器从存储恢复");
}

/// 规格「边界解析」：空白正文不产生消息，也不移动 active leaf。
#[tokio::test]
async fn rejects_blank_message_text() {
    let harness = Harness::start().await;
    let session = harness.session("workspace").await;

    for text in ["", "   ", "\n\t "] {
        let response = harness.submit(&session.id, text).await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(error_code(response).await, agent::ErrorCode::InvalidMessage);
    }

    let tree = harness.tree(&session.id).await;
    assert_eq!(tree.active_leaf, None);
    assert!(tree.nodes.is_empty());
}

/// 消息只能提交到已存在的 session。
#[tokio::test]
async fn rejects_message_for_missing_session() {
    let harness = Harness::start().await;
    let missing = agent::SessionId::parse("missing").expect("合法 session id");

    let response = harness.submit(&missing, "你好").await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        error_code(response).await,
        agent::ErrorCode::SessionNotFound
    );
}

/// 规格「事实事件」：消息追加写入 `message_appended`，并携带更新后的 session 元数据。
#[tokio::test]
async fn records_message_event() {
    let harness = Harness::start().await;
    let session = harness.session("workspace").await;
    let node: agent::MessageNode = harness
        .submit(&session.id, "记录事件")
        .await
        .json()
        .await
        .expect("解析消息节点");

    let payloads: Vec<String> = sqlx::query_scalar("SELECT event FROM event ORDER BY id")
        .fetch_all(&mut connect(&harness.database).await)
        .await
        .expect("查询事件");
    let event: agent::FactEvent =
        serde_json::from_str(payloads.last().expect("有事件")).expect("解析事件");
    let agent::FactEvent::MessageAppended {
        session: recorded,
        message,
    } = event
    else {
        panic!("最后一条事件是 message_appended");
    };
    assert_eq!(message, node);
    assert_eq!(recorded.id, session.id);
    assert_eq!(recorded.updated_at, node.created_at);
}

/// 规格「删除 session」：消息树与历史事件一并删除。
#[tokio::test]
async fn deletes_messages_with_session() {
    let harness = Harness::start().await;
    let session = harness.session("workspace").await;
    harness.submit(&session.id, "待删除").await;

    let response = harness
        .client
        .post(harness.url(&format!("/api/sessions/{}/delete", session.id.as_str())))
        .send()
        .await
        .expect("删除 session");
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let response = harness
        .client
        .get(harness.url(&format!("/api/sessions/{}/tree", session.id.as_str())))
        .send()
        .await
        .expect("读取消息树");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM message")
        .fetch_one(&mut connect(&harness.database).await)
        .await
        .expect("统计消息行");
    assert_eq!(remaining, 0, "session 删除后不保留消息行");
}
