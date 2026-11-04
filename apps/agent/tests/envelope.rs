#![expect(clippy::expect_used, reason = "测试用 expect 直接报告夹具与断言失败。")]

use reqwest::StatusCode;

mod support;

use support::{
    Harness, connect, error_code, error_envelope, recorded_event_kinds, recorded_event_rows,
    recorded_events, recorded_facts,
};

fn missing_id() -> agent::SessionId {
    agent::SessionId::parse("missing").expect("合法 session id")
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
    // 启动先写 host 的两条事实：登记本机 host，再宣告它可用。
    assert_eq!(rows.len(), 4);

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

    let events: Vec<agent::FactEvent> = recorded_events(&harness.database)
        .await
        .into_iter()
        .filter(|event| {
            !matches!(
                event,
                agent::FactEvent::HostRegistered { .. }
                    | agent::FactEvent::HostStatusChanged { .. }
            )
        })
        .collect();
    assert_eq!(
        events,
        vec![
            agent::FactEvent::WorkspaceRegistered {
                workspace: workspace.clone(),
            },
            agent::FactEvent::SessionCreated {
                session_id: session.id.clone(),
            },
        ]
    );

    let facts = recorded_facts(&harness.database).await;
    let sessions: Vec<Option<agent::SessionMetadata>> =
        facts.into_iter().map(|fact| fact.session).collect();
    assert_eq!(
        sessions,
        vec![None, None, None, Some(session)],
        "信封的 session 视图只在属于 session 的事实上出现"
    );
}

/// 存储值不符合领域约束时以 internal 显式失败。
#[tokio::test]
async fn reports_corrupted_stored_metadata_as_internal_error() {
    let harness = Harness::start().await;
    let (_, workspace) = harness.registered_workspace("workspace").await;
    {
        let mut connection = connect(&harness.database).await;
        sqlx::query(
            "INSERT INTO session (id, workspace_host, workspace, title, created_at, updated_at)
             VALUES (?1, ?2, ?3, '', 0, 0)",
        )
        .bind("corrupted")
        .bind(workspace.host.as_str())
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
    assert_eq!(bootstrap.hosts.len(), 1, "启动即登记本机 host");
    assert_eq!(bootstrap.hosts[0].name.as_str(), agent::LOCAL_HOST);
    assert_eq!(bootstrap.hosts[0].status, agent::HostStatus::Ok);
    assert_eq!(bootstrap.max_event_id, 2, "两条 host 事实");
    assert!(bootstrap.workspaces.is_empty());
    assert!(bootstrap.sessions.is_empty());
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
        vec![
            "host_registered",
            "host_status_changed",
            "workspace_registered",
            "session_created",
            "session_renamed"
        ]
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
            "host_registered",
            "host_status_changed",
            "workspace_registered",
            "session_deleted",
            "workspace_removed"
        ]
    );
}
