use std::path::{Path, PathBuf};
use std::time::Duration;

use reqwest::{Response, StatusCode};
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

    async fn create_session(
        &self,
        workspace: &agent::WorkspacePath,
        title: Option<&str>,
    ) -> Response {
        self.create_session_raw(workspace.as_str(), title).await
    }

    async fn create_session_raw(&self, workspace: &str, title: Option<&str>) -> Response {
        self.client
            .post(self.url("/api/sessions"))
            .json(&serde_json::json!({ "workspace": workspace, "title": title }))
            .send()
            .await
            .expect("创建 session")
    }

    async fn rename_session(&self, id: &agent::SessionId, title: &str) -> Response {
        self.client
            .post(self.url(&format!("/api/sessions/{}/rename", id.as_str())))
            .json(&serde_json::json!({ "title": title }))
            .send()
            .await
            .expect("重命名 session")
    }

    async fn delete_session(&self, id: &agent::SessionId) -> Response {
        self.client
            .post(self.url(&format!("/api/sessions/{}/delete", id.as_str())))
            .send()
            .await
            .expect("删除 session")
    }

    async fn session_tree(&self, id: &agent::SessionId) -> Response {
        self.client
            .get(self.url(&format!("/api/sessions/{}/tree", id.as_str())))
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

    async fn created_session(&self, workspace: &agent::WorkspacePath) -> agent::SessionMetadata {
        let response = self.create_session(workspace, None).await;
        assert_eq!(response.status(), StatusCode::CREATED);
        response.json().await.expect("解析 session 元数据")
    }
}

fn missing_id() -> agent::SessionId {
    agent::SessionId::parse("missing").expect("合法 session id")
}

fn recorded_event_ids(database: &Path) -> Vec<i64> {
    let connection = rusqlite::Connection::open(database).expect("打开 SQLite");
    let mut statement = connection
        .prepare("SELECT id FROM event ORDER BY id")
        .expect("准备事件查询");
    let rows = statement
        .query_map([], |row| row.get::<_, i64>(0))
        .expect("查询事件");
    rows.map(|row| row.expect("读取事件 id")).collect()
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
    assert_eq!(registered.path.as_str(), canonical.to_string_lossy());

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

    let response = harness.remove(workspace.path.as_str()).await;
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
    assert_eq!(created.title.as_str(), "新 session");
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
    assert_eq!(renamed.title.as_str(), "重命名后的标题");
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
        harness.session_tree(&missing_id()).await,
        harness.rename_session(&missing_id(), "标题").await,
        harness.delete_session(&missing_id()).await,
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

/// 规格 user story 1：「以规范化绝对路径作为身份。重复注册返回显式错误。」
#[tokio::test]
async fn treats_workspace_identity_as_canonical_path() {
    let harness = Harness::start().await;
    let workspace = harness.workspace("workspace");
    let canonical = std::fs::canonicalize(&workspace).expect("规范化路径");
    assert_eq!(
        harness
            .register(&canonical.to_string_lossy())
            .await
            .status(),
        StatusCode::CREATED
    );

    let dotdot = canonical
        .join("..")
        .join(canonical.file_name().expect("workspace 目录有最后一段路径"));
    let response = harness.register(&dotdot.to_string_lossy()).await;
    assert_eq!(
        response.status(),
        StatusCode::CONFLICT,
        "含 .. 的同一目录视为重复"
    );
    assert_eq!(
        error_code(response).await,
        agent::ErrorCode::WorkspaceDuplicate
    );

    let link = harness.directory.path().join("link-to-workspace");
    std::os::unix::fs::symlink(&canonical, &link).expect("创建符号链接");
    let response = harness.register(&link.to_string_lossy()).await;
    assert_eq!(
        response.status(),
        StatusCode::CONFLICT,
        "符号链接解析后视为重复"
    );
    assert_eq!(
        error_code(response).await,
        agent::ErrorCode::WorkspaceDuplicate
    );

    let response = harness.register("relative/workspace").await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        error_code(response).await,
        agent::ErrorCode::WorkspacePathInvalid
    );

    let bootstrap = harness.bootstrap().await;
    assert_eq!(bootstrap.workspaces.len(), 1);
    assert_eq!(
        bootstrap.workspaces[0].path.as_str(),
        canonical.to_string_lossy()
    );
}

/// 规格「协议」：「事实事件持久化：…workspace 注册/移除」。
#[tokio::test]
async fn records_workspace_removal_as_fact_event() {
    let harness = Harness::start().await;
    let (_, workspace) = harness.registered_workspace("workspace").await;

    assert_eq!(
        harness.remove(workspace.path.as_str()).await.status(),
        StatusCode::NO_CONTENT
    );

    assert_eq!(
        recorded_events(&harness.database),
        vec![
            agent::FactEvent::WorkspaceRegistered {
                workspace: workspace.clone(),
            },
            agent::FactEvent::WorkspaceRemoved {
                path: workspace.path.clone(),
            },
        ]
    );
    let bootstrap = harness.bootstrap().await;
    assert!(bootstrap.workspaces.is_empty());
    assert_eq!(
        bootstrap.max_event_id,
        *recorded_event_ids(&harness.database)
            .last()
            .expect("有事件"),
        "bootstrap 的最大事件 id 等于事件表最大值"
    );
}

/// 规格 user story 2：移除被拒绝后，删除该 workspace 的 session 即可移除。
#[tokio::test]
async fn removes_workspace_after_its_sessions_are_deleted() {
    let harness = Harness::start().await;
    let (_, workspace) = harness.registered_workspace("workspace").await;
    let session = harness.created_session(&workspace.path).await;

    assert_eq!(
        harness.remove(workspace.path.as_str()).await.status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        harness.delete_session(&session.id).await.status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        harness.remove(workspace.path.as_str()).await.status(),
        StatusCode::NO_CONTENT
    );

    let bootstrap = harness.bootstrap().await;
    assert!(bootstrap.workspaces.is_empty());
    assert!(bootstrap.sessions.is_empty());
}

/// 规格 user story 4 与 6：重命名更新标题与更新时间，并保持其余元数据。
#[tokio::test]
async fn rename_updates_updated_at_and_preserves_session_metadata() {
    let harness = Harness::start().await;
    let (_, workspace) = harness.registered_workspace("workspace").await;
    let created = harness.created_session(&workspace.path).await;
    tokio::time::sleep(Duration::from_millis(5)).await;

    let response = harness.rename_session(&created.id, "新的标题").await;
    assert_eq!(response.status(), StatusCode::OK);
    let renamed: agent::SessionMetadata = response.json().await.expect("解析 session 元数据");

    assert_eq!(renamed.id, created.id);
    assert_eq!(renamed.workspace, workspace.path);
    assert_eq!(renamed.title.as_str(), "新的标题");
    assert!(renamed.updated_at > created.updated_at, "更新时间必须前进");
    assert_eq!(renamed.queue_length, created.queue_length);
    assert_eq!(renamed.busy, created.busy);

    let bootstrap = harness.bootstrap().await;
    assert_eq!(bootstrap.sessions, vec![renamed.clone()]);
    let tree: agent::SessionTree = harness
        .session_tree(&created.id)
        .await
        .json()
        .await
        .expect("解析消息树");
    assert_eq!(tree.session, renamed);
}

/// 规格「协议」：全局事件 id 单调递增，重启后继续递增；删除在重启后保持。
#[tokio::test]
async fn keeps_event_ids_monotonic_and_deletion_after_restart() {
    let harness = Harness::start().await;
    let (_, workspace) = harness.registered_workspace("workspace").await;
    let deleted = harness.created_session(&workspace.path).await;
    let kept = harness.created_session(&workspace.path).await;
    let response = harness.rename_session(&kept.id, "保留的标题").await;
    let renamed: agent::SessionMetadata = response.json().await.expect("解析 session 元数据");
    assert_eq!(
        harness.delete_session(&deleted.id).await.status(),
        StatusCode::NO_CONTENT
    );
    let before = harness.bootstrap().await;

    let events = recorded_events(&harness.database);
    assert!(
        !events.contains(&agent::FactEvent::SessionCreated {
            session: deleted.clone(),
        }),
        "删除只清除该 session 的事件"
    );
    assert!(
        events.iter().any(|event| matches!(
            event,
            agent::FactEvent::SessionCreated { session } if session.id == kept.id
        )),
        "其他 session 的事件保留"
    );

    let harness = harness.restart().await;
    let after = harness.bootstrap().await;
    assert_eq!(after.sessions, vec![renamed]);
    assert_eq!(after.workspaces, before.workspaces);
    assert_eq!(after.max_event_id, before.max_event_id);
    assert_eq!(
        harness.session_tree(&deleted.id).await.status(),
        StatusCode::NOT_FOUND,
        "删除在重启后保持"
    );

    let (_, second) = harness.registered_workspace("second").await;
    let bumped = harness.bootstrap().await;
    let ids = recorded_event_ids(&harness.database);
    assert!(
        ids.windows(2).all(|pair| pair[0] < pair[1]),
        "事件 id 严格递增: {ids:?}"
    );
    assert_eq!(
        bumped.max_event_id,
        *ids.last().expect("有事件"),
        "重启后新事件 id 继续递增"
    );
    assert!(bumped.max_event_id > after.max_event_id);
    assert_eq!(bumped.workspaces.len(), 2);
    assert_eq!(bumped.workspaces[1], second);
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

/// 规格 user story 3：workspace 身份是规范化路径，任何拼写都解析到同一身份。
#[tokio::test]
async fn resolves_workspace_path_spellings_for_session_creation() {
    let harness = Harness::start().await;
    let (directory, workspace) = harness.registered_workspace("workspace").await;

    let trailing = format!("{}/", workspace.path.as_str());
    let response = harness.create_session_raw(&trailing, None).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let session: agent::SessionMetadata = response.json().await.expect("解析 session 元数据");
    assert_eq!(session.workspace, workspace.path);

    let link = directory.join("workspace-link");
    std::os::unix::fs::symlink(&directory, &link).expect("创建符号链接");
    let response = harness
        .create_session_raw(&link.to_string_lossy(), None)
        .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let session: agent::SessionMetadata = response.json().await.expect("解析 session 元数据");
    assert_eq!(session.workspace, workspace.path);
}

/// 规格「协议」：错误响应包含稳定的错误码，请求体解析失败也走同一信封。
#[tokio::test]
async fn rejects_unparsable_request_body_with_error_envelope() {
    let harness = Harness::start().await;

    let missing_field = harness
        .client
        .post(harness.url("/api/workspaces"))
        .json(&serde_json::json!({}))
        .send()
        .await
        .expect("发送缺少字段的请求体");
    assert_eq!(missing_field.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        error_code(missing_field).await,
        agent::ErrorCode::InvalidRequest
    );

    let malformed = harness
        .client
        .post(harness.url("/api/workspaces"))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body("{")
        .send()
        .await
        .expect("发送非法 JSON");
    assert_eq!(malformed.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        error_code(malformed).await,
        agent::ErrorCode::InvalidRequest
    );
}

/// 规格 user story 6：session 按最近更新时间排序。
#[tokio::test]
async fn orders_sessions_by_recent_update() {
    let harness = Harness::start().await;
    let (_, workspace) = harness.registered_workspace("workspace").await;
    let first = harness.created_session(&workspace.path).await;
    tokio::time::sleep(Duration::from_millis(5)).await;
    let second = harness.created_session(&workspace.path).await;
    tokio::time::sleep(Duration::from_millis(5)).await;
    let third = harness.created_session(&workspace.path).await;

    let ids = |sessions: &[agent::SessionMetadata]| {
        sessions
            .iter()
            .map(|session| session.id.clone())
            .collect::<Vec<_>>()
    };
    let bootstrap = harness.bootstrap().await;
    assert_eq!(
        ids(&bootstrap.sessions),
        vec![third.id.clone(), second.id.clone(), first.id.clone()],
        "最新创建的 session 排在最前"
    );

    tokio::time::sleep(Duration::from_millis(5)).await;
    assert_eq!(
        harness.rename_session(&first.id, "最近更新").await.status(),
        StatusCode::OK
    );
    let bootstrap = harness.bootstrap().await;
    assert_eq!(
        ids(&bootstrap.sessions)[0],
        first.id,
        "重命名把 session 提到最前"
    );
}
