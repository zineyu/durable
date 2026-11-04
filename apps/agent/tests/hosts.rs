//! 规格 user story R-11、R-17、R-20…R-26：host 是领域对象，workspace 绑定 host，
//! 路径校验发生在 host 上，host 的状态由 link 与心跳驱动。
//!
//! 失效场景用假的 executor 脚本模拟：退出码 3 表示协议不符，被杀表示链路断开。

#![expect(clippy::expect_used, reason = "测试用 expect 直接报告夹具与断言失败。")]

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::Duration;

use axum::http::StatusCode;

mod support;

use support::{Harness, error_code};

/// 写一个假的 executor：`exit <code>`，其余帧一律不读。
fn stub_executor(exit_code: i32) -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().expect("创建临时目录");
    let path = directory.path().join("executor");
    std::fs::write(&path, format!("#!/bin/sh\nexit {exit_code}\n")).expect("写入假 executor");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("加可执行位");
    (directory, path)
}

#[tokio::test]
async fn local_host_is_registered_at_startup() {
    let harness = Harness::start().await;
    let host = harness
        .host_snapshot("local", |host| host.status == agent::HostStatus::Ok)
        .await;
    assert_eq!(host.kind, agent::HostKind::Local, "默认 host 是本机型");
    assert!(host.enabled, "默认 host 启用");
    assert!(host.target.is_none(), "本机 host 没有 SSH 目标");
    assert!(host.detail.is_none(), "ok 时没有说明");
}

#[tokio::test]
async fn host_commands_reject_bad_names_and_targets() {
    let harness = Harness::start().await;

    assert_eq!(
        harness
            .register_host("duplicate", "local", None)
            .await
            .status(),
        StatusCode::CREATED
    );
    let taken = harness.register_host("duplicate", "local", None).await;
    assert_eq!(
        error_code(taken).await,
        agent::ErrorCode::HostNameTaken,
        "重名被拒绝"
    );

    let empty = harness.register_host("  ", "local", None).await;
    assert_eq!(error_code(empty).await, agent::ErrorCode::HostNameInvalid);

    let blank_target = harness.register_host("remote", "ssh", Some("   ")).await;
    assert_eq!(
        error_code(blank_target).await,
        agent::ErrorCode::HostInvalidTarget,
        "空目标被拒绝"
    );

    let option_like = harness
        .register_host("remote", "ssh", Some("-oProxyCommand=evil"))
        .await;
    assert_eq!(
        error_code(option_like).await,
        agent::ErrorCode::HostInvalidTarget,
        "看起来像 ssh 选项的目标被拒绝"
    );

    let unknown = harness
        .update_host("missing", serde_json::json!({ "enabled": false }))
        .await;
    assert_eq!(error_code(unknown).await, agent::ErrorCode::HostNotFound);
}

#[tokio::test]
async fn workspace_registration_uses_the_host_filesystem() {
    let harness = Harness::start().await;

    let missing = harness.register("/definitely/missing/directory").await;
    assert_eq!(
        error_code(missing).await,
        agent::ErrorCode::WorkspacePathInvalid,
        "不存在的路径在 host 上被拒绝"
    );

    let file = harness.workspace_path("file");
    std::fs::write(&file, "x").expect("写入普通文件");
    let not_directory = harness.register(file.to_str().expect("路径是 UTF-8")).await;
    assert_eq!(
        error_code(not_directory).await,
        agent::ErrorCode::WorkspacePathInvalid,
        "不是目录的路径被拒绝"
    );
}

#[tokio::test]
async fn the_same_path_can_belong_to_two_hosts() {
    let harness = Harness::start().await;
    let raw = "/tmp/durable-shared-workspace";
    std::fs::create_dir_all(raw).expect("创建共享目录");
    let canonical = std::fs::canonicalize(raw).expect("规范化目录");
    let path = canonical.to_str().expect("路径是 UTF-8");

    assert_eq!(
        harness.register_host("twin", "local", None).await.status(),
        StatusCode::CREATED
    );
    assert_eq!(
        harness.register_on("local", path).await.status(),
        StatusCode::CREATED
    );
    assert_eq!(
        harness.register_on("twin", path).await.status(),
        StatusCode::CREATED
    );

    let duplicate = harness.register_on("twin", path).await;
    assert_eq!(
        error_code(duplicate).await,
        agent::ErrorCode::WorkspaceDuplicate,
        "同一 host 上不能重复注册"
    );

    let bootstrap = harness.bootstrap().await;
    let hosts: Vec<&str> = bootstrap
        .workspaces
        .iter()
        .filter(|workspace| workspace.path.as_str() == path)
        .map(|workspace| workspace.host.as_str())
        .collect();
    assert_eq!(
        hosts,
        vec!["local", "twin"],
        "同一路径可在两台 host 上各存一份"
    );

    assert_eq!(
        harness.remove_on("twin", path).await.status(),
        StatusCode::NO_CONTENT
    );
    let after = harness.bootstrap().await;
    assert_eq!(
        after
            .workspaces
            .iter()
            .filter(|workspace| workspace.path.as_str() == path)
            .count(),
        1,
        "移除只影响那一台 host"
    );
}

#[tokio::test]
async fn a_host_with_workspaces_cannot_be_removed() {
    let harness = Harness::start().await;
    let raw = "/tmp/durable-host-in-use";
    std::fs::create_dir_all(raw).expect("创建目录");
    let canonical = std::fs::canonicalize(raw).expect("规范化目录");
    let path = canonical.to_str().expect("路径是 UTF-8");
    assert_eq!(
        harness.register_host("busy", "local", None).await.status(),
        StatusCode::CREATED
    );
    assert_eq!(
        harness.register_on("busy", path).await.status(),
        StatusCode::CREATED
    );

    let in_use = harness.remove_host("busy").await;
    assert_eq!(error_code(in_use).await, agent::ErrorCode::HostInUse);

    assert_eq!(
        harness.remove_on("busy", path).await.status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        harness.remove_host("busy").await.status(),
        StatusCode::NO_CONTENT,
        "清空 workspace 之后可以删除"
    );
    let bootstrap = harness.bootstrap().await;
    assert!(
        bootstrap
            .hosts
            .iter()
            .all(|host| host.name.as_str() != "busy"),
        "删除后 host 表里没有它"
    );
}

#[tokio::test]
async fn protocol_mismatch_marks_the_host_incompatible() {
    let (_directory, executor) = stub_executor(3);
    let harness = Harness::start_with_executor(executor, Duration::from_millis(50)).await;

    let host = harness
        .host_snapshot("local", |host| {
            host.status == agent::HostStatus::Incompatible
        })
        .await;
    assert!(
        host.detail.is_some_and(|detail| !detail.is_empty()),
        "不兼容状态带说明"
    );
}

#[tokio::test]
async fn killed_executor_marks_the_host_unreachable_and_reconnect_restores_it() {
    let harness = Harness::start().await;
    harness
        .host_snapshot("local", |host| host.status == agent::HostStatus::Ok)
        .await;
    let pid = harness.executor_pid();

    support::kill_process(pid);

    let unreachable = harness
        .host_snapshot("local", |host| {
            host.status == agent::HostStatus::Unreachable
        })
        .await;
    assert!(
        unreachable.detail.is_some_and(|detail| !detail.is_empty()),
        "不可达状态带说明"
    );

    assert_eq!(
        harness.reconnect_host("local").await.status(),
        StatusCode::OK
    );
    harness
        .host_snapshot("local", |host| host.status == agent::HostStatus::Ok)
        .await;
}

/// 规格 user story R-8：停用 host 关闭它的 link，子进程不留在后台。
#[tokio::test]
async fn disabling_a_host_closes_its_link() {
    let harness = Harness::start().await;
    harness
        .host_snapshot("local", |host| host.status == agent::HostStatus::Ok)
        .await;
    let pid = harness.executor_pid();

    let response = harness
        .update_host("local", serde_json::json!({ "enabled": false }))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let host = harness
        .host_snapshot("local", |host| {
            host.status == agent::HostStatus::Unreachable
        })
        .await;
    assert_eq!(host.detail.as_deref(), Some("host 已停用"));

    for _ in 0..100 {
        if !process_alive(pid) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("停用后 executor 子进程仍然活着: {pid}");
}

/// 进程是否仍存在。仅测试使用，不引入额外依赖。
fn process_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[tokio::test]
async fn heartbeat_records_latency() {
    let harness = Harness::start_with_executor(
        support::process::executor_binary(),
        Duration::from_millis(50),
    )
    .await;

    let host = harness
        .host_snapshot("local", |host| host.latency_ms.is_some())
        .await;
    assert!(host.latency_ms.is_some(), "心跳后记录往返毫秒数");
}

#[tokio::test]
async fn renaming_a_host_moves_its_workspaces_and_sessions() {
    let harness = Harness::start().await;
    let raw = "/tmp/durable-host-rename";
    std::fs::create_dir_all(raw).expect("创建目录");
    let canonical = std::fs::canonicalize(raw).expect("规范化目录");
    let path = canonical.to_str().expect("路径是 UTF-8");

    assert_eq!(
        harness
            .register_host("before", "local", None)
            .await
            .status(),
        StatusCode::CREATED
    );
    assert_eq!(
        harness.register_on("before", path).await.status(),
        StatusCode::CREATED
    );
    assert_eq!(
        harness
            .create_session_on("before", path, None)
            .await
            .status(),
        StatusCode::CREATED
    );

    assert_eq!(
        harness
            .update_host("before", serde_json::json!({ "name": "after" }))
            .await
            .status(),
        StatusCode::OK
    );

    let bootstrap = harness.bootstrap().await;
    assert!(
        bootstrap
            .hosts
            .iter()
            .all(|host| host.name.as_str() != "before"),
        "旧名立即成为未知 host"
    );
    let workspace = bootstrap
        .workspaces
        .iter()
        .find(|workspace| workspace.path.as_str() == path)
        .expect("workspace 仍在");
    assert_eq!(workspace.host.as_str(), "after", "workspace 跟着改名");
    let session = bootstrap
        .sessions
        .iter()
        .find(|entry| entry.workspace.as_str() == path)
        .expect("session 仍在");
    assert_eq!(session.host.as_str(), "after", "session 的 host 也跟着改名");
}

#[tokio::test]
async fn the_local_host_reconnects_without_a_manual_click() {
    let harness = Harness::start().await;
    harness
        .host_snapshot("local", |host| host.status == agent::HostStatus::Ok)
        .await;
    support::kill_process(harness.executor_pid());

    harness
        .host_snapshot("local", |host| {
            host.status == agent::HostStatus::Unreachable
        })
        .await;
    harness
        .host_snapshot("local", |host| host.status == agent::HostStatus::Ok)
        .await;
}
