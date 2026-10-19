use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};

use reqwest::{Response, StatusCode};
use tempfile::TempDir;

struct Harness {
    directory: TempDir,
    database: PathBuf,
    base_url: String,
    handle: tokio::task::JoinHandle<()>,
    client: reqwest::Client,
}

impl Harness {
    async fn start() -> Harness {
        let directory = tempfile::tempdir().expect("创建临时目录");
        let database = directory.path().join("agent.sqlite");
        Harness::open(directory, database).await
    }

    async fn open(directory: TempDir, database: PathBuf) -> Harness {
        let store = agent::Store::open(&database).expect("打开 SQLite");
        let listener = tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .await
            .expect("绑定回环端口");
        let address = listener.local_addr().expect("读取本地地址");
        let handle = tokio::spawn(async move {
            agent::serve(
                listener,
                agent::AgentState::new(store, "test-model".to_owned()),
            )
            .await
            .expect("agent 服务运行");
        });
        Harness {
            directory,
            database,
            base_url: format!("http://{address}"),
            handle,
            client: reqwest::Client::new(),
        }
    }

    async fn restart(self) -> Harness {
        let Harness {
            directory,
            database,
            handle,
            ..
        } = self;
        handle.abort();
        let _ = handle.await;
        Harness::open(directory, database).await
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    fn workspace(&self, name: &str) -> PathBuf {
        let path = self.directory.path().join(name);
        std::fs::create_dir(&path).expect("创建 workspace 目录");
        path
    }

    async fn register(&self, path: &str) -> Response {
        self.client
            .post(self.url("/api/workspaces"))
            .json(&serde_json::json!({ "path": path }))
            .send()
            .await
            .expect("注册 workspace")
    }

    async fn remove(&self, path: &str) -> Response {
        self.client
            .post(self.url("/api/workspaces/remove"))
            .json(&serde_json::json!({ "path": path }))
            .send()
            .await
            .expect("移除 workspace")
    }

    async fn bootstrap(&self) -> agent::BootstrapResponse {
        self.client
            .get(self.url("/api/bootstrap"))
            .send()
            .await
            .expect("读取 bootstrap")
            .json()
            .await
            .expect("解析 bootstrap")
    }

    async fn create_session(&self, workspace: &str, title: Option<&str>) -> Response {
        self.client
            .post(self.url("/api/sessions"))
            .json(&serde_json::json!({ "workspace": workspace, "title": title }))
            .send()
            .await
            .expect("创建 session")
    }

    async fn rename_session(&self, id: &str, title: &str) -> Response {
        self.client
            .post(self.url(&format!("/api/sessions/{id}/rename")))
            .json(&serde_json::json!({ "title": title }))
            .send()
            .await
            .expect("重命名 session")
    }

    async fn delete_session(&self, id: &str) -> Response {
        self.client
            .post(self.url(&format!("/api/sessions/{id}/delete")))
            .send()
            .await
            .expect("删除 session")
    }

    async fn session_tree(&self, id: &str) -> Response {
        self.client
            .get(self.url(&format!("/api/sessions/{id}/tree")))
            .send()
            .await
            .expect("读取消息树")
    }

    async fn registered_workspace(&self, name: &str) -> (PathBuf, agent::WorkspaceMetadata) {
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

    async fn created_session(&self, workspace: &str) -> agent::SessionMetadata {
        let response = self.create_session(workspace, None).await;
        assert_eq!(response.status(), StatusCode::CREATED);
        response.json().await.expect("解析 session 元数据")
    }
}

fn recorded_events(database: &Path) -> Vec<agent::FactEvent> {
    let connection = rusqlite::Connection::open(database).expect("打开 SQLite");
    let mut statement = connection
        .prepare("SELECT event FROM event ORDER BY id")
        .expect("准备事件查询");
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .expect("查询事件");
    rows.map(|row| serde_json::from_str(&row.expect("读取事件")).expect("解析事件"))
        .collect()
}

async fn error_code(response: Response) -> agent::ErrorCode {
    response
        .json::<agent::ErrorResponse>()
        .await
        .expect("解析错误响应")
        .code
}

#[tokio::test]
async fn rejects_workspace_paths_that_are_not_directories() {
    let harness = Harness::start().await;

    let missing = harness.directory.path().join("missing");
    let response = harness.register(&missing.to_string_lossy()).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        error_code(response).await,
        agent::ErrorCode::WorkspacePathInvalid
    );

    let file = harness.directory.path().join("datei.txt");
    std::fs::write(&file, "内容").expect("写入文件");
    let response = harness.register(&file.to_string_lossy()).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        error_code(response).await,
        agent::ErrorCode::WorkspacePathInvalid
    );

    assert!(harness.bootstrap().await.workspaces.is_empty());
}

#[tokio::test]
async fn registers_workspace_with_canonical_path() {
    let harness = Harness::start().await;
    let workspace = harness.workspace("workspace");
    let trailing = format!("{}/", workspace.to_string_lossy());

    let response = harness.register(&trailing).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let registered: agent::WorkspaceMetadata = response.json().await.expect("解析注册响应");
    let canonical = std::fs::canonicalize(&workspace).expect("规范化路径");
    assert_eq!(registered.path, canonical.to_string_lossy());

    let bootstrap = harness.bootstrap().await;
    assert_eq!(bootstrap.model, "test-model");
    assert_eq!(bootstrap.workspaces, vec![registered]);
    assert_eq!(bootstrap.max_event_id, 1);
}

#[tokio::test]
async fn rejects_duplicate_workspace_registration() {
    let harness = Harness::start().await;
    let workspace = harness.workspace("workspace");
    let path = workspace.to_string_lossy().into_owned();

    assert_eq!(harness.register(&path).await.status(), StatusCode::CREATED);

    let response = harness.register(&path).await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(
        error_code(response).await,
        agent::ErrorCode::WorkspaceDuplicate
    );
    assert_eq!(harness.bootstrap().await.workspaces.len(), 1);
}

#[tokio::test]
async fn removes_workspace_registration_without_touching_disk() {
    let harness = Harness::start().await;
    let workspace = harness.workspace("workspace");
    let marker = workspace.join("keep.txt");
    std::fs::write(&marker, "内容").expect("写入标记文件");
    let path = std::fs::canonicalize(&workspace)
        .expect("规范化路径")
        .to_string_lossy()
        .into_owned();

    assert_eq!(harness.register(&path).await.status(), StatusCode::CREATED);
    let response = harness.remove(&path).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    assert!(harness.bootstrap().await.workspaces.is_empty());
    assert!(marker.exists(), "移除注册不得删除目录内容");
}

#[tokio::test]
async fn rejects_removing_unregistered_workspace() {
    let harness = Harness::start().await;
    let workspace = harness.workspace("workspace");

    let response = harness.remove(&workspace.to_string_lossy()).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        error_code(response).await,
        agent::ErrorCode::WorkspaceNotFound
    );
}

#[tokio::test]
async fn rejects_removing_workspace_that_still_has_sessions() {
    let harness = Harness::start().await;
    let (_, workspace) = harness.registered_workspace("workspace").await;
    let session = harness.created_session(&workspace.path).await;

    let response = harness.remove(&workspace.path).await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(
        error_code(response).await,
        agent::ErrorCode::WorkspaceNotEmpty
    );

    let bootstrap = harness.bootstrap().await;
    assert_eq!(bootstrap.workspaces, vec![workspace]);
    assert_eq!(bootstrap.sessions, vec![session]);
}

#[tokio::test]
async fn records_session_lifecycle_events_and_keeps_metadata_consistent() {
    let harness = Harness::start().await;
    let (_, workspace) = harness.registered_workspace("workspace").await;
    let created = harness.created_session(&workspace.path).await;
    assert_eq!(created.title, "新 session");
    assert_eq!(created.workspace, workspace.path);
    assert!(!created.busy);
    assert_eq!(created.queue_length, 0);

    let bootstrap = harness.bootstrap().await;
    assert_eq!(bootstrap.max_event_id, 2);
    assert_eq!(bootstrap.sessions, vec![created.clone()]);

    let tree: agent::SessionTree = harness
        .session_tree(&created.id)
        .await
        .json()
        .await
        .expect("解析消息树");
    assert_eq!(tree.session, created);
    assert_eq!(tree.active_leaf, None);
    assert!(tree.nodes.is_empty());

    let response = harness.rename_session(&created.id, "重命名后的标题").await;
    assert_eq!(response.status(), StatusCode::OK);
    let renamed: agent::SessionMetadata = response.json().await.expect("解析 session 元数据");
    assert_eq!(renamed.title, "重命名后的标题");
    assert!(renamed.updated_at >= created.updated_at);
    assert_eq!(harness.bootstrap().await.sessions, vec![renamed.clone()]);
    let tree: agent::SessionTree = harness
        .session_tree(&created.id)
        .await
        .json()
        .await
        .expect("解析消息树");
    assert_eq!(tree.session, renamed);

    assert_eq!(
        recorded_events(&harness.database),
        vec![
            agent::FactEvent::WorkspaceRegistered {
                workspace: workspace.clone(),
            },
            agent::FactEvent::SessionCreated {
                session: created.clone(),
            },
            agent::FactEvent::SessionRenamed { session: renamed },
        ]
    );

    let response = harness.delete_session(&created.id).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let bootstrap = harness.bootstrap().await;
    assert!(bootstrap.sessions.is_empty());
    assert_eq!(bootstrap.max_event_id, 4);
    assert_eq!(
        harness.session_tree(&created.id).await.status(),
        StatusCode::NOT_FOUND
    );

    assert_eq!(
        recorded_events(&harness.database),
        vec![
            agent::FactEvent::WorkspaceRegistered {
                workspace: workspace.clone(),
            },
            agent::FactEvent::SessionDeleted {
                session_id: created.id,
            },
        ],
        "删除 session 清除其事件，只保留删除事实"
    );
}

#[tokio::test]
async fn rejects_unknown_session() {
    let harness = Harness::start().await;

    for response in [
        harness.session_tree("missing").await,
        harness.rename_session("missing", "标题").await,
        harness.delete_session("missing").await,
    ] {
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            error_code(response).await,
            agent::ErrorCode::SessionNotFound
        );
    }
}

#[tokio::test]
async fn rejects_blank_session_title() {
    let harness = Harness::start().await;
    let (_, workspace) = harness.registered_workspace("workspace").await;

    let response = harness.create_session(&workspace.path, Some("   ")).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(error_code(response).await, agent::ErrorCode::InvalidTitle);

    let session = harness.created_session(&workspace.path).await;
    let response = harness.rename_session(&session.id, "  ").await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(error_code(response).await, agent::ErrorCode::InvalidTitle);
    assert_eq!(harness.bootstrap().await.sessions, vec![session]);
}

#[tokio::test]
async fn restores_workspaces_sessions_and_titles_after_restart() {
    let harness = Harness::start().await;
    let (_, workspace) = harness.registered_workspace("workspace").await;
    let created = harness.created_session(&workspace.path).await;
    let response = harness.rename_session(&created.id, "重启前的标题").await;
    let renamed: agent::SessionMetadata = response.json().await.expect("解析 session 元数据");
    let before = harness.bootstrap().await;

    let harness = harness.restart().await;
    let after = harness.bootstrap().await;
    assert_eq!(after, before);
    assert_eq!(after.workspaces, vec![workspace]);
    assert_eq!(after.sessions, vec![renamed.clone()]);

    let tree: agent::SessionTree = harness
        .session_tree(&created.id)
        .await
        .json()
        .await
        .expect("解析消息树");
    assert_eq!(tree.session, renamed);
}

#[tokio::test]
async fn deleting_session_does_not_touch_workspace() {
    let harness = Harness::start().await;
    let (directory, workspace) = harness.registered_workspace("workspace").await;
    let marker = directory.join("keep.txt");
    std::fs::write(&marker, "内容").expect("写入标记文件");
    let session = harness.created_session(&workspace.path).await;

    assert_eq!(
        harness.delete_session(&session.id).await.status(),
        StatusCode::NO_CONTENT
    );
    assert!(marker.exists(), "删除 session 不得改动工作区文件");
    assert!(directory.exists(), "删除 session 不得删除工作区目录");
}
