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
