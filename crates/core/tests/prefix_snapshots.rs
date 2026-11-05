//! 提交点副本：`Store::open_recording` 在每个提交点留下一份可独立打开的库。
//! 崩溃前缀矩阵靠它枚举崩溃点，见 ADR 0056。

mod support;

use support::{add_session, text};

use durable::{HostKind, MessageText, Store, SubmitMode};

/// 每个提交点都有一份副本，且每份副本都能打开、通过不变量检查。
#[tokio::test]
async fn every_commit_leaves_a_reopenable_copy() {
    let directory = tempfile::tempdir().expect("创建临时目录");
    let snapshots = directory.path().join("prefixes");
    let store = Store::open_recording(&directory.path().join("store.sqlite"), &snapshots)
        .await
        .expect("打开数据库");

    store
        .register_host(support::local_host(), HostKind::Local, None)
        .await
        .expect("注册 host");
    store
        .register_workspace(&support::local_host(), &support::work_path())
        .await
        .expect("注册 workspace");
    let session = add_session(&store).await;
    store
        .enqueue_message(&session, text("第一句"), SubmitMode::Queue)
        .await
        .expect("入队");

    let committed = store.commits();
    let mut names: Vec<String> = std::fs::read_dir(&snapshots)
        .expect("读取副本目录")
        .map(|entry| {
            entry
                .expect("目录项")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    assert_eq!(
        names.len() as u64,
        committed,
        "每个提交点一份副本: {names:?}"
    );
    assert_eq!(
        names.first().map(String::as_str),
        Some("prefix-000001.sqlite")
    );
    assert_eq!(
        names.last().map(String::as_str),
        Some(format!("prefix-{committed:06}.sqlite").as_str())
    );

    for name in names {
        let copy = Store::open(&snapshots.join(&name))
            .await
            .expect("副本可以独立打开");
        assert!(
            copy.check_invariants()
                .await
                .expect("检查不变量")
                .is_empty(),
            "{name} 的不变量应当自洽"
        );
        assert_eq!(copy.commits(), 0, "副本不记录新的提交点");
    }
}

/// 最后一个副本就是当前库的状态：同样的读取结果。
#[tokio::test]
async fn the_last_copy_matches_the_live_store() {
    let directory = tempfile::tempdir().expect("创建临时目录");
    let snapshots = directory.path().join("prefixes");
    let store = Store::open_recording(&directory.path().join("store.sqlite"), &snapshots)
        .await
        .expect("打开数据库");
    store
        .register_host(support::local_host(), HostKind::Local, None)
        .await
        .expect("注册 host");
    store
        .register_workspace(&support::local_host(), &support::work_path())
        .await
        .expect("注册 workspace");
    let session = add_session(&store).await;
    store
        .enqueue_message(
            &session,
            MessageText::parse("一句话").expect("合法正文"),
            SubmitMode::Queue,
        )
        .await
        .expect("入队");

    let copy = Store::open(&snapshots.join(format!("prefix-{:06}.sqlite", store.commits())))
        .await
        .expect("打开最后一个副本");
    let live = store.bootstrap().await.expect("读取当前库");
    let copied = copy.bootstrap().await.expect("读取副本");
    assert_eq!(live.sessions, copied.sessions);
    assert_eq!(live.workspaces, copied.workspaces);
    assert_eq!(live.max_event_id, copied.max_event_id);
}

/// 副本写不出来时提交照旧成功，但副本目录留下一个空洞，矩阵的点数对账因此报红。
///
/// 这里用「目标文件已存在」确定性地制造一次复制失败，不依赖磁盘满或权限；生产路径的
/// 行为不变：事务已经提交，副本缺失不让写命令失败（ADR 0062）。
#[tokio::test]
async fn a_copy_that_cannot_be_written_leaves_a_hole() {
    let directory = tempfile::tempdir().expect("创建临时目录");
    let snapshots = directory.path().join("prefixes");
    std::fs::create_dir_all(&snapshots).expect("建立副本目录");
    let placeholder = snapshots.join("prefix-000001.sqlite");
    std::fs::write(&placeholder, b"placeholder").expect("写入占位文件");

    let store = Store::open_recording(&directory.path().join("store.sqlite"), &snapshots)
        .await
        .expect("打开数据库");
    store
        .register_host(support::local_host(), HostKind::Local, None)
        .await
        .expect("注册 host");

    assert_eq!(store.commits(), 1, "提交照旧成功");
    let content = std::fs::read(&placeholder).expect("读回占位文件");
    assert_eq!(content, b"placeholder", "复制失败不覆盖已经存在的文件");

    let copies: Vec<String> = std::fs::read_dir(&snapshots)
        .expect("读取副本目录")
        .map(|entry| {
            entry
                .expect("目录项")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(
        copies,
        vec!["prefix-000001.sqlite"],
        "序号 1 没有可用的副本"
    );
    assert!(
        Store::open(&placeholder).await.is_err(),
        "占位文件不是数据库，因此这一份副本打不开：矩阵的点表会比提交次数少"
    );
}
