use durable::Store;

/// 数据库文件的父目录不存在时自动建立，而不是返回 SQLite 打开失败。
#[tokio::test]
async fn creates_missing_database_directory() {
    let directory = tempfile::tempdir().expect("创建临时目录");
    let path = directory
        .path()
        .join("state")
        .join("durable")
        .join("agent.sqlite");

    let store = Store::open(&path).await.expect("打开嵌套路径的数据库");
    assert!(path.exists(), "数据库文件应被建立");
    assert_eq!(store.hosts().await.expect("读取 host 表").len(), 0);

    drop(store);
    Store::open(&path).await.expect("再次打开已存在的数据库");
}
