use std::path::Path;

use reqwest::StatusCode;

mod support;

use support::{Harness, error_code};

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
    assert_eq!(
        bootstrap.max_event_id, 3,
        "启动的两条 host 事实之后才是这次注册"
    );
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

/// 规格 user story A-1：「以规范化绝对路径作为身份。重复注册返回显式错误。」
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

/// 规格 user story A-2：移除被拒绝后，删除该 workspace 的 session 即可移除。
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

/// 规格 user story A-3：workspace 身份是规范化路径，任何拼写都解析到同一身份。
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

/// 规格 user story A-3：workspace 身份是规范化路径，点段拼写也解析到同一身份。
#[tokio::test]
async fn resolves_dot_segment_spellings_for_session_creation() {
    let harness = Harness::start().await;
    let (_, workspace) = harness.registered_workspace("workspace").await;
    let name = Path::new(workspace.path.as_str())
        .file_name()
        .expect("workspace 路径有最后一段")
        .to_string_lossy()
        .into_owned();

    for spelling in [
        format!("{}/.", workspace.path.as_str()),
        format!("{}/../{}", workspace.path.as_str(), name),
    ] {
        let response = harness.create_session_raw(&spelling, None).await;
        assert_eq!(response.status(), StatusCode::CREATED, "拼写: {spelling}");
        let session: agent::SessionMetadata = response.json().await.expect("解析 session 元数据");
        assert_eq!(session.workspace, workspace.path);
    }
}

/// 规格「持久化」：移除 workspace 只删除注册记录，目录被删除后仍可移除。
#[tokio::test]
async fn removes_workspace_registration_after_directory_is_deleted() {
    let harness = Harness::start().await;
    let (directory, workspace) = harness.registered_workspace("workspace").await;
    std::fs::remove_dir_all(&directory).expect("删除 workspace 目录");

    assert_eq!(
        harness.remove(workspace.path.as_str()).await.status(),
        StatusCode::NO_CONTENT
    );
    assert!(harness.bootstrap().await.workspaces.is_empty());
}

/// 规格 user story A-1 与「持久化」：重复注册在重启后仍被拒绝，且不写事件。
#[tokio::test]
async fn duplicate_workspace_registration_survives_restart() {
    let harness = Harness::start().await;
    let (_, workspace) = harness.registered_workspace("workspace").await;
    let before = harness.bootstrap().await;

    let harness = harness.restart().await;
    let response = harness.register(workspace.path.as_str()).await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(
        error_code(response).await,
        agent::ErrorCode::WorkspaceDuplicate
    );

    let after = harness.bootstrap().await;
    assert_eq!(after.workspaces, before.workspaces);
    assert_eq!(
        after.max_event_id,
        before.max_event_id + 1,
        "被拒绝的注册不写 workspace 事实，只有一条 host 状态事实"
    );
}

/// 规格「错误响应…workspace 未注册」：移除后再移除报告缺失。
#[tokio::test]
async fn repeated_workspace_removal_reports_missing_registration() {
    let harness = Harness::start().await;
    let (_, workspace) = harness.registered_workspace("workspace").await;

    assert_eq!(
        harness.remove(workspace.path.as_str()).await.status(),
        StatusCode::NO_CONTENT
    );
    let response = harness.remove(workspace.path.as_str()).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        error_code(response).await,
        agent::ErrorCode::WorkspaceNotFound
    );
}

/// 规格「错误码至少区分 workspace 路径无效」：移除非绝对路径是路径错误，不是「未注册」。
#[tokio::test]
async fn removing_workspace_with_relative_path_is_invalid() {
    let harness = Harness::start().await;

    let response = harness.remove("relative/workspace").await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        error_code(response).await,
        agent::ErrorCode::WorkspacePathInvalid
    );
}

/// 路径无法读取时给出与「不存在」不同的原因。
#[tokio::test]
async fn reports_unreadable_workspace_path_with_its_own_reason() {
    let harness = Harness::start().await;
    let file = harness.directory.path().join("datei.txt");
    std::fs::write(&file, "内容").expect("写入文件");

    let response = harness
        .register(&file.join("child").to_string_lossy())
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body: agent::ErrorResponse = response.json().await.expect("解析错误响应");
    assert_eq!(body.code, agent::ErrorCode::WorkspacePathInvalid);
    assert!(
        body.message.contains("无法读取"),
        "message: {}",
        body.message
    );
}
