//! 迁移 0006 的升级路径：旧库把内嵌的工具调用拆成 tool 节点。
//! 新建数据库的路径在 `store_open`，行映射与查询在 `crates/core/src/storage`。

#![expect(
    clippy::expect_used,
    reason = "升级路径的夹具与断言用 expect 直接报告失败，测试里不需要恢复。"
)]

use std::path::PathBuf;

use durable::{ContentPart, MessageRole, NodeName, SessionId, Store};
use sqlx::migrate::Migrator;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

/// 0006 之前的迁移。升级路径从这些版本开始。
const LEGACY_MIGRATIONS: [(&str, &str); 5] = [
    ("0001_init.sql", include_str!("../migrations/0001_init.sql")),
    (
        "0002_queue.sql",
        include_str!("../migrations/0002_queue.sql"),
    ),
    ("0003_run.sql", include_str!("../migrations/0003_run.sql")),
    (
        "0004_branch.sql",
        include_str!("../migrations/0004_branch.sql"),
    ),
    ("0005_node.sql", include_str!("../migrations/0005_node.sql")),
];

/// 旧库：一条用户消息、一条带两个工具调用的 assistant 消息、一条其后的用户消息。
/// 消息 id 1、2、3 已用，下一个可用 id 是 5；active leaf 是最后那条用户消息。
const LEGACY_SEED: &str = "
    INSERT INTO workspace (path, registered_at) VALUES ('/work', 1);
    INSERT INTO session (id, workspace, title, created_at, updated_at)
        VALUES ('s-1', '/work', '标题', 1, 1);
    INSERT INTO run (id, session_id, status, failure, created_at, updated_at)
        VALUES (1, 's-1', 'succeeded', NULL, 1, 1);
    INSERT INTO message (id, session_id, parent_id, role, text, created_at, run_id, tool_calls)
        VALUES (1, 's-1', NULL, 'user', '读文件', 1, NULL, '[]'),
               (2, 's-1', 1, 'assistant', '这里说明。', 2, 1,
                '[{\"id\":\"c1\",\"name\":\"read_file\",\"arguments\":\"{}\",
                   \"outcome\":{\"status\":\"succeeded\",\"content\":\"ok\"}},
                  {\"id\":\"c2\",\"name\":\"bash\",\"arguments\":\"{}\",
                   \"outcome\":{\"status\":\"failed\",\"code\":\"bash_timeout\",\"message\":\"超时\"}}]'),
               (3, 's-1', 2, 'user', '后续', 3, NULL, '[]');
    UPDATE meta SET value = 5 WHERE key = 'next_message_id';
    UPDATE session SET active_leaf = 3 WHERE id = 's-1';
";

/// 用 0001–0005 建库、写入旧数据，再交给 [`Store::open`] 应用 0006。
async fn legacy_database(directory: &tempfile::TempDir) -> PathBuf {
    database_with(directory, &LEGACY_MIGRATIONS, LEGACY_SEED).await
}

/// 0008 之前的全部迁移。
const NODE_MIGRATIONS: [(&str, &str); 7] = [
    ("0001_init.sql", include_str!("../migrations/0001_init.sql")),
    (
        "0002_queue.sql",
        include_str!("../migrations/0002_queue.sql"),
    ),
    ("0003_run.sql", include_str!("../migrations/0003_run.sql")),
    (
        "0004_branch.sql",
        include_str!("../migrations/0004_branch.sql"),
    ),
    ("0005_node.sql", include_str!("../migrations/0005_node.sql")),
    (
        "0006_message_parts.sql",
        include_str!("../migrations/0006_message_parts.sql"),
    ),
    (
        "0007_steer.sql",
        include_str!("../migrations/0007_steer.sql"),
    ),
];

/// 0008 之前的节点表只登记名称、URL 与注册时间。
const NODE_SEED: &str = "INSERT INTO node (name, url, created_at)
     VALUES ('laptop', 'http://127.0.0.1:18081', 1234);";

/// 0009 之前的全部迁移。
const SESSION_MIGRATIONS: [(&str, &str); 8] = [
    ("0001_init.sql", include_str!("../migrations/0001_init.sql")),
    (
        "0002_queue.sql",
        include_str!("../migrations/0002_queue.sql"),
    ),
    ("0003_run.sql", include_str!("../migrations/0003_run.sql")),
    (
        "0004_branch.sql",
        include_str!("../migrations/0004_branch.sql"),
    ),
    ("0005_node.sql", include_str!("../migrations/0005_node.sql")),
    (
        "0006_message_parts.sql",
        include_str!("../migrations/0006_message_parts.sql"),
    ),
    (
        "0007_steer.sql",
        include_str!("../migrations/0007_steer.sql"),
    ),
    (
        "0008_node_ops.sql",
        include_str!("../migrations/0008_node_ops.sql"),
    ),
];

/// 旧事件把 session 元数据放在载荷里：一个带视图的 session 事实、一个不带视图的删除事实、
/// 一个 workspace 事实。
const SESSION_SEED: &str = r#"
    INSERT INTO workspace (path, registered_at) VALUES ('/work', 1);
    INSERT INTO session (id, workspace, title, created_at, updated_at)
        VALUES ('s-1', '/work', '标题', 1, 1);
    INSERT INTO event (session_id, created_at, event) VALUES
        ('s-1', 5,
         '{"kind":"session_renamed","session":{"id":"s-1","workspace":"/work","title":"标题","busy":true,"queueLength":2,"updatedAt":7}}'),
        ('s-1', 6, '{"kind":"session_deleted","sessionId":"s-1"}'),
        ('s-1', 7, '{"kind":"queue_item_removed","sessionId":"s-1","itemId":3}'),
        (NULL, 8, '{"kind":"workspace_removed","path":"/work"}');
"#;

/// 用给定迁移建库、写入种子数据，再交给 [`Store::open`] 应用剩余迁移。
async fn database_with(
    directory: &tempfile::TempDir,
    migrations: &[(&str, &str)],
    seed: &'static str,
) -> PathBuf {
    let migrations_directory = directory.path().join("migrations");
    std::fs::create_dir(&migrations_directory).expect("创建迁移目录");
    for (name, sql) in migrations {
        std::fs::write(migrations_directory.join(name), sql).expect("写出旧迁移");
    }
    let path = directory.path().join("legacy.sqlite");
    let options = SqliteConnectOptions::new()
        .filename(&path)
        .create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .expect("打开旧库");
    Migrator::new(migrations_directory.as_path())
        .await
        .expect("读取旧迁移")
        .run(&pool)
        .await
        .expect("应用旧迁移");
    sqlx::raw_sql(seed)
        .execute(&pool)
        .await
        .expect("写入旧数据");
    pool.close().await;
    path
}

/// 迁移在既有库上升级后节点表内容不变，`enabled` 为 true，`updated_at` 取原注册时间。
#[tokio::test]
async fn upgrades_node_table_with_enabled_and_updated_at() {
    let directory = tempfile::tempdir().expect("创建临时目录");
    let path = database_with(&directory, &NODE_MIGRATIONS, NODE_SEED).await;
    let store = Store::open(&path).await.expect("升级旧库");
    let name = NodeName::parse("laptop").expect("合法节点名");

    let node = store
        .node(&name)
        .await
        .expect("读取节点")
        .expect("升级后节点仍在");
    assert_eq!(node.url, "http://127.0.0.1:18081");
    assert_eq!(node.created_at, 1234);
    assert!(node.enabled, "旧节点升级后默认启用");
    assert_eq!(node.updated_at, 1234, "更新时间从注册时间补齐");
}

/// 规格 A-8：旧库升级后工具调用成为 tool 节点，transcript 仍是链上顺序。
#[tokio::test]
async fn splits_legacy_tool_calls_into_tool_nodes() {
    let directory = tempfile::tempdir().expect("创建临时目录");
    let path = legacy_database(&directory).await;
    let store = Store::open(&path).await.expect("升级旧库");
    let session = SessionId::parse("s-1").expect("合法 session id");

    let transcript = store.transcript(&session).await.expect("读取 transcript");
    let chain: Vec<(i64, MessageRole)> = transcript
        .iter()
        .map(|node| (node.id.get(), node.role))
        .collect();
    assert_eq!(
        chain,
        vec![
            (1, MessageRole::User),
            (2, MessageRole::Assistant),
            (5, MessageRole::Tool),
            (6, MessageRole::Tool),
            (3, MessageRole::User),
        ],
        "工具节点串在 assistant 下，其后的消息挂到最后一个工具节点，顺序按链而不是按 id"
    );

    let assistant = transcript.get(1).expect("assistant 节点");
    let calls: Vec<String> = assistant
        .tool_calls()
        .map(|(call_id, name, _)| format!("{call_id}:{name}"))
        .collect();
    assert_eq!(
        calls,
        vec!["c1:read_file".to_owned(), "c2:bash".to_owned()],
        "调用块保持原顺序"
    );
    assert_eq!(assistant.text(), "这里说明。", "旧正文成为文本块");

    let outcomes: Vec<String> = transcript
        .iter()
        .filter(|node| node.role == MessageRole::Tool)
        .flat_map(durable::MessageNode::tool_results)
        .map(|(call_id, state)| match state {
            durable::ToolResultState::Finished { outcome } => match outcome {
                durable::ToolOutcome::Succeeded { content } => format!("{call_id}:{content}"),
                durable::ToolOutcome::Failed { code, .. } => format!("{call_id}:{code:?}"),
            },
            other => format!("{call_id}:{other:?}"),
        })
        .collect();
    assert_eq!(
        outcomes,
        vec!["c1:ok".to_owned(), "c2:BashTimeout".to_owned()]
    );

    let tool_node = transcript.get(2).expect("第一个工具节点");
    assert!(
        tool_node
            .parts
            .iter()
            .all(|part| matches!(part, ContentPart::ToolResult { .. })),
        "工具节点只带工具结果块"
    );
    assert_eq!(
        tool_node.run_id,
        Some(durable::RunId::parse(1).expect("合法 run id")),
        "工具节点继承所属 run"
    );

    let next_id = store.mint_message_id().await.expect("铸造消息 id");
    assert_eq!(next_id.get(), 7, "分配器跳过新铸的工具节点 id");
}

/// 规格 A-21：骨架读取只取块的类型、状态与字节数，不读正文。
#[tokio::test]
async fn reads_skeletons_without_bodies() {
    let directory = tempfile::tempdir().expect("创建临时目录");
    let path = legacy_database(&directory).await;
    let store = Store::open(&path).await.expect("升级旧库");
    let session = SessionId::parse("s-1").expect("合法 session id");

    let tree = store.session_tree(&session).await.expect("读取消息树");
    let assistant = tree
        .nodes
        .iter()
        .find(|node| node.id.get() == 2)
        .expect("assistant 节点");
    let kinds: Vec<durable::ContentKind> = assistant.parts.iter().map(|part| part.kind).collect();
    assert_eq!(
        kinds,
        vec![
            durable::ContentKind::Text,
            durable::ContentKind::ToolCall,
            durable::ContentKind::ToolCall,
        ],
        "骨架保留块序与类型"
    );
    assert!(
        assistant.parts.iter().all(|part| part.size > 0),
        "骨架带块字节数"
    );
    assert_eq!(tree.active_leaf.map(durable::MessageId::get), Some(3));
}

/// 迁移把旧载荷里的 session 元数据搬到事件信封上，并从载荷中摘除。
#[tokio::test]
async fn moves_session_view_from_payload_to_envelope() {
    let directory = tempfile::tempdir().expect("创建临时目录");
    let path = database_with(&directory, &SESSION_MIGRATIONS, SESSION_SEED).await;
    let store = Store::open(&path).await.expect("升级旧库");
    let session = SessionId::parse("s-1").expect("合法 session id");

    let events = store.events_after(0).await.expect("回放事件");
    assert_eq!(events.len(), 4);

    let renamed = events.first().expect("改名事件");
    assert_eq!(
        renamed.event,
        durable::FactEvent::SessionRenamed {
            session_id: session.clone(),
        },
        "载荷只留事实身份"
    );
    let view = renamed
        .session
        .as_ref()
        .expect("旧事件的信封带 session 视图");
    assert_eq!(view.id, session);
    assert_eq!(view.title.as_str(), "标题");
    assert!(view.busy, "旧载荷的 busy 原样保留");
    assert_eq!(view.queue_length, 2, "旧载荷的队列长度原样保留");
    assert_eq!(view.updated_at, 7, "旧载荷的更新时间原样保留");

    assert!(
        events.iter().skip(1).all(|event| event.session.is_none()),
        "不带视图的事实升级后仍不带视图"
    );
    assert_eq!(
        events.get(2).map(|event| &event.event),
        Some(&durable::FactEvent::QueueItemRemoved {
            session_id: session.clone(),
            item_id: durable::QueueItemId::parse(3).expect("合法队列项 id"),
        }),
        "旧队列项移除事实保留，视图为空"
    );

    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(SqliteConnectOptions::new().filename(&path))
        .await
        .expect("打开升级后的库");
    let payloads: Vec<String> = sqlx::query_scalar("SELECT event FROM event ORDER BY id")
        .fetch_all(&pool)
        .await
        .expect("读取事件载荷");
    let payload: serde_json::Value = serde_json::from_str(&payloads[0]).expect("解析事件载荷");
    assert!(
        payload.get("session").is_none(),
        "旧载荷不再含 session 键: {payload}"
    );
    assert_eq!(
        payload.get("sessionId").and_then(|id| id.as_str()),
        Some("s-1"),
        "session 身份留在载荷里"
    );
}
