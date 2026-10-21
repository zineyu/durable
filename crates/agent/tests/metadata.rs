#![expect(
    clippy::expect_used,
    reason = "元数据辅助函数用 expect 直接报告夹具与断言失败。"
)]

use std::path::Path;
use std::time::Duration;

use reqwest::StatusCode;

mod support;

use support::{
    Harness, connect, error_code, error_envelope, recorded_event_ids, recorded_event_kinds,
    recorded_event_rows, recorded_events,
};

fn missing_id() -> agent::SessionId {
    agent::SessionId::parse("missing").expect("合法 session id")
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
        recorded_events(&harness.database).await,
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
        recorded_events(&harness.database).await,
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
        recorded_events(&harness.database).await,
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
            .await
            .last()
            .expect("有事件"),
        "bootstrap 的最大事件 id 等于事件表最大值"
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

/// 规格 user story A-4 与 A-6：重命名更新标题与更新时间，并保持其余元数据。
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

    let events = recorded_events(&harness.database).await;
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
    let ids = recorded_event_ids(&harness.database).await;
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

/// 规格 user story A-6：session 按最近更新时间排序。
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

/// 规格「协议」：请求体无法解析时也返回稳定的错误码、说明与 HTTP 状态。
#[tokio::test]
async fn rejects_request_body_type_failures_with_error_envelope() {
    let harness = Harness::start().await;

    let wrong_field_type = harness
        .client
        .post(harness.url("/api/workspaces"))
        .json(&serde_json::json!({ "path": 42 }))
        .send()
        .await
        .expect("发送字段类型错误的请求体");
    assert_eq!(wrong_field_type.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let envelope = error_envelope(wrong_field_type).await;
    assert_eq!(envelope.code, agent::ErrorCode::InvalidRequest);
    assert!(!envelope.message.trim().is_empty(), "说明不得为空");

    let wrong_media_type = harness
        .client
        .post(harness.url("/api/workspaces"))
        .header(reqwest::header::CONTENT_TYPE, "text/plain")
        .body("path=/tmp")
        .send()
        .await
        .expect("发送错误媒体类型的请求体");
    assert_eq!(
        wrong_media_type.status(),
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
    let envelope = error_envelope(wrong_media_type).await;
    assert_eq!(envelope.code, agent::ErrorCode::InvalidRequest);
    assert!(!envelope.message.trim().is_empty(), "说明不得为空");

    let (_, workspace) = harness.registered_workspace("workspace").await;
    let session = harness.created_session(&workspace.path).await;
    let rename_wrong_type = harness
        .client
        .post(harness.url(&format!("/api/sessions/{}/rename", session.id.as_str())))
        .json(&serde_json::json!({ "title": 7 }))
        .send()
        .await
        .expect("发送字段类型错误的重命名请求体");
    assert_eq!(rename_wrong_type.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let envelope = error_envelope(rename_wrong_type).await;
    assert_eq!(envelope.code, agent::ErrorCode::InvalidRequest);
    assert!(!envelope.message.trim().is_empty(), "说明不得为空");
}

/// 规格「协议」：错误响应包含稳定的错误码、人类可读说明与 HTTP 状态。
#[tokio::test]
async fn error_responses_carry_code_status_and_message() {
    let harness = Harness::start().await;
    let (_, workspace) = harness.registered_workspace("workspace").await;
    let session = harness.created_session(&workspace.path).await;
    let other = harness.workspace("other");
    let other_path = other.to_string_lossy().into_owned();

    let cases = vec![
        (
            harness.register("relative/path").await,
            StatusCode::UNPROCESSABLE_ENTITY,
            agent::ErrorCode::WorkspacePathInvalid,
        ),
        (
            harness.register(workspace.path.as_str()).await,
            StatusCode::CONFLICT,
            agent::ErrorCode::WorkspaceDuplicate,
        ),
        (
            harness.remove(workspace.path.as_str()).await,
            StatusCode::CONFLICT,
            agent::ErrorCode::WorkspaceNotEmpty,
        ),
        (
            harness.create_session_raw(&other_path, None).await,
            StatusCode::NOT_FOUND,
            agent::ErrorCode::WorkspaceNotFound,
        ),
        (
            harness.session_tree(&missing_id()).await,
            StatusCode::NOT_FOUND,
            agent::ErrorCode::SessionNotFound,
        ),
        (
            harness.rename_session(&session.id, "  ").await,
            StatusCode::UNPROCESSABLE_ENTITY,
            agent::ErrorCode::InvalidTitle,
        ),
    ];

    for (response, status, code) in cases {
        assert_eq!(response.status(), status, "错误类别 {code:?} 的状态码");
        let envelope = error_envelope(response).await;
        assert_eq!(envelope.code, code);
        assert!(
            !envelope.message.trim().is_empty(),
            "说明不得为空: {:?}",
            envelope.code
        );
    }
}

/// 规格「协议」：事实事件以 id、`session_id`、时间戳与负载持久化。
#[tokio::test]
async fn persists_fact_event_envelope_fields() {
    let harness = Harness::start().await;
    let (_, workspace) = harness.registered_workspace("workspace").await;
    let session = harness.created_session(&workspace.path).await;

    let rows = recorded_event_rows(&harness.database).await;
    assert_eq!(rows.len(), 2);

    let mut previous_id = 0;
    for (id, session_id, created_at, payload) in rows {
        assert!(id > previous_id, "事件 id 严格递增");
        previous_id = id;
        assert!(created_at > 0, "时间戳必须存在");
        let event: agent::FactEvent = serde_json::from_str(&payload).expect("解析事件负载");
        assert_eq!(
            session_id.as_deref(),
            event.session_id().map(agent::SessionId::as_str),
            "session_id 列必须等于负载所属 session"
        );
    }

    assert_eq!(
        recorded_events(&harness.database).await,
        vec![
            agent::FactEvent::WorkspaceRegistered { workspace },
            agent::FactEvent::SessionCreated { session },
        ]
    );
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
/// 规格「协议」：非法 session id 是请求错误，不是「不存在」。
#[tokio::test]
async fn rejects_blank_session_id_as_invalid_request() {
    let harness = Harness::start().await;

    let response = harness
        .client
        .get(harness.url("/api/sessions/%20/tree"))
        .send()
        .await
        .expect("请求消息树");
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(error_code(response).await, agent::ErrorCode::InvalidRequest);
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

/// 存储值不符合领域约束时以 internal 显式失败。
#[tokio::test]
async fn reports_corrupted_stored_metadata_as_internal_error() {
    let harness = Harness::start().await;
    let (_, workspace) = harness.registered_workspace("workspace").await;
    {
        let mut connection = connect(&harness.database).await;
        sqlx::query(
            "INSERT INTO session (id, workspace, title, created_at, updated_at)
             VALUES (?1, ?2, '', 0, 0)",
        )
        .bind("corrupted")
        .bind(workspace.path.as_str())
        .execute(&mut connection)
        .await
        .expect("写入空标题的 session 行");
    }

    let response = harness
        .client
        .get(harness.url("/api/bootstrap"))
        .send()
        .await
        .expect("请求 bootstrap");
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(error_code(response).await, agent::ErrorCode::Internal);
}

/// 规格「读取 bootstrap 元数据」：空数据库返回空列表与最大事件 id 0。
#[tokio::test]
async fn empty_bootstrap_reports_no_metadata_and_zero_events() {
    let harness = Harness::start().await;

    let bootstrap = harness.bootstrap().await;
    assert_eq!(bootstrap.model, "test-model");
    assert_eq!(bootstrap.max_event_id, 0);
    assert!(bootstrap.workspaces.is_empty());
    assert!(bootstrap.sessions.is_empty());
}

/// 规格「协议」：session tree 快照携带水位，从该游标回放的是快照之后的事件。
#[tokio::test]
async fn session_tree_reports_watermark_and_replays_events_after_it() {
    let harness = Harness::start().await;
    let (_, workspace) = harness.registered_workspace("workspace").await;
    let session = harness.created_session(&workspace.path).await;

    let tree = harness.tree(&session.id).await;
    let watermark = tree.max_event_id;
    assert_eq!(
        watermark,
        harness.bootstrap().await.max_event_id,
        "水位等于快照读取时的最大事件 id"
    );

    let response = harness.rename_session(&session.id, "新标题").await;
    assert_eq!(response.status(), StatusCode::OK);
    let renamed = harness.tree(&session.id).await;
    assert_eq!(
        renamed.max_event_id,
        *recorded_event_ids(&harness.database)
            .await
            .last()
            .expect("有事件")
    );
    assert!(renamed.max_event_id > watermark);

    let mut events = harness.subscribe(watermark).await;
    let replayed = tokio::time::timeout(Duration::from_secs(5), events.next())
        .await
        .expect("等待回放事件")
        .expect("事件流未结束");
    match replayed {
        agent::StreamEvent::Fact { id, .. } => assert_eq!(id, renamed.max_event_id),
        other => panic!("回放段只含事实事件: {other:?}"),
    }
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
        after.max_event_id, before.max_event_id,
        "被拒绝的注册不写事实事件"
    );
}

/// 规格「显式失败」与「事件 id 由数据库自增序列产生」：被拒绝的命令不写事实事件。
#[tokio::test]
async fn rejected_commands_do_not_advance_event_ids() {
    let harness = Harness::start().await;
    let (_, workspace) = harness.registered_workspace("workspace").await;
    let session = harness.created_session(&workspace.path).await;
    let before = harness.bootstrap().await;

    assert_eq!(
        harness.register(workspace.path.as_str()).await.status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        harness.remove(workspace.path.as_str()).await.status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        harness.rename_session(&missing_id(), "标题").await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        harness
            .create_session(&workspace.path, Some("  "))
            .await
            .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        harness.delete_session(&missing_id()).await.status(),
        StatusCode::NOT_FOUND
    );

    let after = harness.bootstrap().await;
    assert_eq!(after.max_event_id, before.max_event_id);
    assert_eq!(after.sessions, vec![session]);
}

/// 规格「错误响应…session 不存在」：删除后再删除报告缺失，而非成功。
#[tokio::test]
async fn repeated_session_deletion_reports_missing_session() {
    let harness = Harness::start().await;
    let (_, workspace) = harness.registered_workspace("workspace").await;
    let session = harness.created_session(&workspace.path).await;

    assert_eq!(
        harness.delete_session(&session.id).await.status(),
        StatusCode::NO_CONTENT
    );
    let response = harness.delete_session(&session.id).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        error_code(response).await,
        agent::ErrorCode::SessionNotFound
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

/// 规格「协议类型由 Rust 定义…生成 TS 类型」：事实事件的 kind 使用客户端 reducer 约定的名字。
#[tokio::test]
async fn persists_event_kind_wire_names() {
    let harness = Harness::start().await;

    let (_, workspace) = harness.registered_workspace("workspace").await;
    let session = harness.created_session(&workspace.path).await;
    assert_eq!(
        harness.rename_session(&session.id, "新标题").await.status(),
        StatusCode::OK
    );
    assert_eq!(
        recorded_event_kinds(&harness.database).await,
        vec!["workspace_registered", "session_created", "session_renamed"]
    );

    assert_eq!(
        harness.delete_session(&session.id).await.status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        harness.remove(workspace.path.as_str()).await.status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        recorded_event_kinds(&harness.database).await,
        vec![
            "workspace_registered",
            "session_deleted",
            "workspace_removed"
        ]
    );
}

/// 规格「边界解析，核心信任」：存储的 session id 非法时以 internal 显式失败。
#[tokio::test]
async fn reports_corrupted_stored_session_id_as_internal_error() {
    let harness = Harness::start().await;
    let (_, workspace) = harness.registered_workspace("workspace").await;
    {
        let mut connection = connect(&harness.database).await;
        sqlx::query(
            "INSERT INTO session (id, workspace, title, created_at, updated_at)
             VALUES (?1, ?2, ?3, 0, 0)",
        )
        .bind("   ")
        .bind(workspace.path.as_str())
        .bind("标题")
        .execute(&mut connection)
        .await
        .expect("写入空 id 的 session 行");
    }

    let response = harness
        .client
        .get(harness.url("/api/bootstrap"))
        .send()
        .await
        .expect("请求 bootstrap");
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(error_code(response).await, agent::ErrorCode::Internal);
}

/// 规格「协议」：路径参数无法解码时也返回错误信封。
#[tokio::test]
async fn rejects_undecodable_session_path_with_error_envelope() {
    let harness = Harness::start().await;

    let response = harness
        .client
        .get(harness.url("/api/sessions/%FF/tree"))
        .send()
        .await
        .expect("请求消息树");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(error_code(response).await, agent::ErrorCode::InvalidRequest);
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
