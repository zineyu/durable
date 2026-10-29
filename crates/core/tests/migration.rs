//! 旧库的打开路径：结构不是基线时以显式错误拒绝，不迁移也不导入。
//! 新库的建库路径在 `store_open`，行映射与查询在 `crates/core/src/storage`。

#![expect(
    clippy::expect_used,
    reason = "旧库夹具与断言用 expect 直接报告失败，测试里不需要恢复。"
)]

use std::path::PathBuf;

use durable::{Store, StoreError};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

/// 多节点时期的完整表结构：workspace 没有 host 列，另有 node 表。
const LEGACY_SCHEMA: &str = "
    CREATE TABLE node (
        name TEXT PRIMARY KEY,
        url TEXT NOT NULL,
        enabled INTEGER NOT NULL DEFAULT 1,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    );
    CREATE TABLE workspace (
        path TEXT PRIMARY KEY,
        registered_at INTEGER NOT NULL
    );
    CREATE TABLE session (
        id TEXT PRIMARY KEY,
        workspace TEXT NOT NULL REFERENCES workspace(path),
        title TEXT NOT NULL,
        title_auto INTEGER NOT NULL DEFAULT 1,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        active_leaf INTEGER
    );
    INSERT INTO workspace (path, registered_at) VALUES ('/work', 1);
    INSERT INTO session (id, workspace, title, created_at, updated_at)
        VALUES ('s-1', '/work', '标题', 1, 1);
";

/// 旧库的迁移记录：多节点时期是 10 条。
const LEGACY_MIGRATIONS: &str = "
    CREATE TABLE _sqlx_migrations (
        version BIGINT PRIMARY KEY,
        description TEXT NOT NULL,
        installed_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
        success BOOLEAN NOT NULL,
        checksum BLOB NOT NULL,
        execution_time BIGINT NOT NULL
    );
    INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time)
        VALUES (1, 'init', 1, x'00', 0),
               (5, 'node', 1, x'00', 0),
               (10, 'progress', 1, x'00', 0);
";

/// 迁移记录不是基线这一条：版本 1 的描述是 `init`。
const FOREIGN_HISTORY: &str = "
    CREATE TABLE host (name TEXT PRIMARY KEY);
    CREATE TABLE workspace (host TEXT NOT NULL, path TEXT NOT NULL);
    CREATE TABLE _sqlx_migrations (
        version BIGINT PRIMARY KEY,
        description TEXT NOT NULL,
        installed_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
        success BOOLEAN NOT NULL,
        checksum BLOB NOT NULL,
        execution_time BIGINT NOT NULL
    );
    INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time)
        VALUES (1, 'init', 1, x'00', 0);
";

/// 用旧结构建库，返回它的路径。
async fn legacy_database(directory: &tempfile::TempDir, scripts: &[&'static str]) -> PathBuf {
    let path = directory.path().join("legacy.sqlite");
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true),
        )
        .await
        .expect("建立旧库");
    for script in scripts {
        for statement in script.split(';') {
            if statement.trim().is_empty() {
                continue;
            }
            sqlx::raw_sql(statement)
                .execute(&pool)
                .await
                .expect("写入旧结构");
        }
    }
    pool.close().await;
    path
}

/// 规格 ADR 0048：旧库不导入，打开即失败，错误说明要求改用新路径。
#[tokio::test]
async fn refuses_legacy_database_with_node_table() {
    let directory = tempfile::tempdir().expect("创建临时目录");
    let path = legacy_database(&directory, &[LEGACY_SCHEMA, LEGACY_MIGRATIONS]).await;

    let Err(error) = Store::open(&path).await else {
        panic!("旧库被拒绝");
    };
    assert!(
        matches!(error, StoreError::LegacyDatabase(ref refused) if *refused == path),
        "得到 {error:?}"
    );
    let message = error.to_string();
    assert!(message.contains("旧结构"), "错误说明指出旧库: {message}");
    assert!(
        message.contains("新的数据库路径"),
        "错误说明要求换路径: {message}"
    );
}

/// 有迁移记录但版本不是基线这一条，同样是旧库。
#[tokio::test]
async fn refuses_database_with_foreign_migration_history() {
    let directory = tempfile::tempdir().expect("创建临时目录");
    let path = legacy_database(&directory, &[FOREIGN_HISTORY]).await;

    let Err(error) = Store::open(&path).await else {
        panic!("迁移记录不符被拒绝");
    };
    assert!(
        matches!(error, StoreError::LegacyDatabase(_)),
        "得到 {error:?}"
    );
}

/// 新库建成基线后重新打开仍然可用：同一路径第二次打开不触发旧库判定。
#[tokio::test]
async fn reopens_a_database_it_created() {
    let directory = tempfile::tempdir().expect("创建临时目录");
    let path = directory.path().join("new.sqlite");
    let store = Store::open(&path).await.expect("建立新库");
    drop(store);

    let store = Store::open(&path).await.expect("重新打开新库");
    assert!(store.hosts().await.expect("读取 host 表").is_empty());
}
