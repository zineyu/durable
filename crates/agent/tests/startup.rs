use std::path::{Path, PathBuf};
use std::process::Command;

mod support;

fn run_agent(directory: &Path, configure: impl FnOnce(&mut Command)) -> std::process::Output {
    let mut command = support::command(&directory.join("agent.sqlite"));
    configure(&mut command);
    command.output().expect("启动 agent 二进制")
}

async fn spawn_agent(directory: &Path, workspaces: &[PathBuf]) -> (tokio::process::Child, String) {
    let mut command = support::serving_command(&directory.join("agent.sqlite"));
    for workspace in workspaces {
        command.arg("--workspace").arg(workspace);
    }
    let mut child = command.spawn().expect("启动 agent");
    let address = support::read_address(&mut child).await;
    (child, address)
}

async fn read_bootstrap(address: &str) -> agent::BootstrapResponse {
    reqwest::get(format!("http://{address}/api/bootstrap"))
        .await
        .expect("请求 bootstrap")
        .json()
        .await
        .expect("解析 bootstrap")
}

/// 规格「进程与部署」：可重复传入初始 workspace 路径，同一路径只注册一次。
#[tokio::test]
async fn starts_with_repeated_initial_workspace() {
    let directory = tempfile::tempdir().expect("创建临时目录");
    let workspace = directory.path().join("workspace");
    std::fs::create_dir(&workspace).expect("创建 workspace 目录");
    let canonical = std::fs::canonicalize(&workspace)
        .expect("规范化路径")
        .to_string_lossy()
        .into_owned();

    let (mut child, address) = spawn_agent(directory.path(), &[workspace.clone(), workspace]).await;
    let bootstrap = read_bootstrap(&address).await;
    assert_eq!(bootstrap.workspaces.len(), 1);
    assert_eq!(bootstrap.workspaces[0].path.as_str(), canonical);
    assert_eq!(bootstrap.model, "test-model");

    child.start_kill().expect("结束 agent");
}

/// 规格「进程与部署」：「agent 缺少必需配置时启动失败，错误信息指出缺失项。」
#[test]
fn fails_when_model_config_is_missing() {
    let directory = tempfile::tempdir().expect("创建临时目录");
    let output = run_agent(directory.path(), |command| {
        command
            .env_remove(agent::MODEL_BASE_URL)
            .env_remove(agent::MODEL_API_KEY)
            .env_remove(agent::MODEL_NAME);
    });

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("stderr 是 UTF-8");
    for name in [
        agent::MODEL_BASE_URL,
        agent::MODEL_API_KEY,
        agent::MODEL_NAME,
    ] {
        assert!(stderr.contains(name), "stderr 缺少 {name}: {stderr}");
    }
}

/// 规格「进程与部署」：缺少必需配置时只列出缺失项。
#[test]
fn fails_when_only_one_model_config_is_missing() {
    let directory = tempfile::tempdir().expect("创建临时目录");
    let output = run_agent(directory.path(), |command| {
        command.env_remove(agent::MODEL_NAME);
    });

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("stderr 是 UTF-8");
    assert!(stderr.contains(agent::MODEL_NAME), "stderr: {stderr}");
    assert!(
        !stderr.contains(agent::MODEL_BASE_URL),
        "已提供的配置不得被列为缺失项: {stderr}"
    );
    assert!(
        !stderr.contains(agent::MODEL_API_KEY),
        "已提供的配置不得被列为缺失项: {stderr}"
    );
}

/// 规格「进程与部署」：初始 workspace 路径无效时启动失败。
#[test]
fn fails_when_initial_workspace_is_missing() {
    let directory = tempfile::tempdir().expect("创建临时目录");
    let output = run_agent(directory.path(), |command| {
        command
            .arg("--workspace")
            .arg(directory.path().join("missing"));
    });

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("stderr 是 UTF-8");
    assert!(stderr.contains("workspace 路径不存在"), "stderr: {stderr}");
}

/// 规格「持久化」：同一 SQLite 上重启后 workspace 恢复，且重复传入同一初始 workspace 不重复注册。
#[tokio::test]
async fn restarts_with_same_initial_workspace_without_duplicating() {
    let directory = tempfile::tempdir().expect("创建临时目录");
    let workspace = directory.path().join("workspace");
    std::fs::create_dir(&workspace).expect("创建 workspace 目录");
    let canonical = std::fs::canonicalize(&workspace)
        .expect("规范化路径")
        .to_string_lossy()
        .into_owned();

    let (mut first, address) =
        spawn_agent(directory.path(), std::slice::from_ref(&workspace)).await;
    let before = read_bootstrap(&address).await;
    assert_eq!(before.workspaces.len(), 1);
    first.start_kill().expect("结束 agent");
    first.wait().await.expect("等待 agent 退出");

    let (mut second, address) = spawn_agent(directory.path(), &[workspace]).await;
    let after = read_bootstrap(&address).await;
    assert_eq!(after.workspaces.len(), 1, "重启不产生重复注册");
    assert_eq!(after.workspaces[0].path.as_str(), canonical);
    assert_eq!(
        after.max_event_id, before.max_event_id,
        "重复传入已注册的初始 workspace 不写事件"
    );
    second.start_kill().expect("结束第二个 agent");
    second.wait().await.expect("等待第二个 agent 退出");
}
/// 规格「进程与部署」：空值视为缺失。
#[test]
fn fails_when_model_config_is_blank() {
    let directory = tempfile::tempdir().expect("创建临时目录");
    let output = run_agent(directory.path(), |command| {
        command.env(agent::MODEL_API_KEY, "  ");
    });

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("stderr 是 UTF-8");
    assert!(stderr.contains(agent::MODEL_API_KEY), "stderr: {stderr}");
    assert!(!stderr.contains(agent::MODEL_NAME), "stderr: {stderr}");
}

/// 规格「进程与部署」：配置不是 UTF-8 与配置缺失是不同失败。
#[cfg(unix)]
#[test]
fn fails_when_model_config_is_not_unicode() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let directory = tempfile::tempdir().expect("创建临时目录");
    let output = run_agent(directory.path(), |command| {
        command.env(agent::MODEL_NAME, OsString::from_vec(vec![0xff, 0xfe]));
    });

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("stderr 是 UTF-8");
    assert!(stderr.contains("配置不是 UTF-8"), "stderr: {stderr}");
    assert!(stderr.contains(agent::MODEL_NAME), "stderr: {stderr}");
}
