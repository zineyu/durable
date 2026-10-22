//! 直接读测试数据库的辅助：事件回放与连接。
//! 通过 HTTP 的辅助在父模块。

#![expect(
    dead_code,
    clippy::expect_used,
    reason = "测试脚手架用 expect 直接报告夹具失败；本模块被每个测试目标各编译一次，未被该目标用到的条目属正常情况。"
)]

use std::path::Path;
use std::time::Duration;

use sqlx::{ConnectOptions, Row};

pub(crate) async fn connect(database: &Path) -> sqlx::SqliteConnection {
    sqlx::sqlite::SqliteConnectOptions::new()
        .filename(database)
        .busy_timeout(Duration::from_secs(5))
        .connect()
        .await
        .expect("打开 SQLite")
}

pub(crate) async fn recorded_event_ids(database: &Path) -> Vec<i64> {
    sqlx::query_scalar("SELECT id FROM event ORDER BY id")
        .fetch_all(&mut connect(database).await)
        .await
        .expect("查询事件 id")
}

pub(crate) async fn recorded_events(database: &Path) -> Vec<agent::FactEvent> {
    let payloads: Vec<String> = sqlx::query_scalar("SELECT event FROM event ORDER BY id")
        .fetch_all(&mut connect(database).await)
        .await
        .expect("查询事件");
    payloads
        .iter()
        .map(|payload| serde_json::from_str(payload).expect("解析事件"))
        .collect()
}

pub(crate) async fn recorded_event_kinds(database: &Path) -> Vec<String> {
    recorded_event_rows(database)
        .await
        .into_iter()
        .map(|(_, _, _, payload)| {
            let event: serde_json::Value = serde_json::from_str(&payload).expect("解析事件负载");
            event
                .get("kind")
                .and_then(serde_json::Value::as_str)
                .expect("事件负载有 kind")
                .to_owned()
        })
        .collect()
}

pub(crate) async fn recorded_event_rows(
    database: &Path,
) -> Vec<(i64, Option<String>, i64, String)> {
    sqlx::query("SELECT id, session_id, created_at, event FROM event ORDER BY id")
        .fetch_all(&mut connect(database).await)
        .await
        .expect("查询事件")
        .iter()
        .map(|row| {
            (
                row.get::<i64, _>(0),
                row.get::<Option<String>, _>(1),
                row.get::<i64, _>(2),
                row.get::<String, _>(3),
            )
        })
        .collect()
}
