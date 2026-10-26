#![expect(clippy::expect_used, reason = "测试用 expect 直接报告夹具与断言失败。")]

use std::time::Duration;

use reqwest::StatusCode;

mod support;

use support::{Harness, connect, error_code, recorded_event_ids, recorded_events};

fn missing_id() -> agent::SessionId {
    agent::SessionId::parse("missing").expect("合法 session id")
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

    let tree: agent::SessionTreeSnapshot = harness
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
    let tree: agent::SessionTreeSnapshot = harness
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
                session_id: created.id.clone(),
            },
            agent::FactEvent::SessionRenamed {
                session_id: renamed.id.clone(),
            },
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

    let tree: agent::SessionTreeSnapshot = harness
        .session_tree(&created.id)
        .await
        .json()
        .await
        .expect("解析消息树");
    assert_eq!(tree.session, renamed);
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
    let tree: agent::SessionTreeSnapshot = harness
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
            session_id: deleted.id.clone(),
        }),
        "删除只清除该 session 的事件"
    );
    assert!(
        events.iter().any(|event| matches!(
            event,
            agent::FactEvent::SessionCreated { session_id } if *session_id == kept.id
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
