use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_agent"))
}

fn set_model_config(command: &mut Command) {
    command
        .env(agent::MODEL_BASE_URL, "http://127.0.0.1:9/v1")
        .env(agent::MODEL_API_KEY, "test-key")
        .env(agent::MODEL_NAME, "test-model");
}

#[tokio::test]
async fn starts_with_repeated_initial_workspace() {
    use tokio::io::AsyncBufReadExt;

    let directory = tempfile::tempdir().expect("创建临时目录");
    let workspace = directory.path().join("workspace");
    std::fs::create_dir(&workspace).expect("创建 workspace 目录");
    let canonical = std::fs::canonicalize(&workspace)
        .expect("规范化路径")
        .to_string_lossy()
        .into_owned();

    let mut command = tokio::process::Command::new(binary());
    command
        .args(["--listen", "127.0.0.1:0", "--database"])
        .arg(directory.path().join("agent.sqlite"))
        .arg("--workspace")
        .arg(&workspace)
        .arg("--workspace")
        .arg(&workspace)
        .env(agent::MODEL_BASE_URL, "http://127.0.0.1:9/v1")
        .env(agent::MODEL_API_KEY, "test-key")
        .env(agent::MODEL_NAME, "test-model")
        .stdout(Stdio::piped())
        .kill_on_drop(true);

    let mut child = command.spawn().expect("启动 agent");
    let stdout = child.stdout.take().expect("取得启动输出");
    let mut lines = tokio::io::BufReader::new(stdout).lines();
    let line = tokio::time::timeout(Duration::from_secs(10), lines.next_line())
        .await
        .expect("等待启动输出")
        .expect("读取启动输出")
        .expect("agent 打印监听地址");
    let address = line
        .strip_prefix("agent listening on http://")
        .expect("启动输出包含监听地址");

    let bootstrap: agent::BootstrapResponse =
        reqwest::get(format!("http://{address}/api/bootstrap"))
            .await
            .expect("请求 bootstrap")
            .json()
            .await
            .expect("解析 bootstrap");
    assert_eq!(bootstrap.workspaces.len(), 1);
    assert_eq!(bootstrap.workspaces[0].path, canonical);
    assert_eq!(bootstrap.model, "test-model");

    child.start_kill().expect("结束 agent");
}

#[test]
fn fails_when_model_config_is_missing() {
    let directory = tempfile::tempdir().expect("创建临时目录");
    let mut command = Command::new(binary());
    command
        .args(["--database"])
        .arg(directory.path().join("agent.sqlite"))
        .env_remove(agent::MODEL_BASE_URL)
        .env_remove(agent::MODEL_API_KEY)
        .env_remove(agent::MODEL_NAME);

    let output = command.output().expect("启动 agent");
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

#[test]
fn fails_when_initial_workspace_is_missing() {
    let directory = tempfile::tempdir().expect("创建临时目录");
    let mut command = Command::new(binary());
    command
        .args(["--database"])
        .arg(directory.path().join("agent.sqlite"))
        .arg("--workspace")
        .arg(directory.path().join("missing"));
    set_model_config(&mut command);

    let output = command.output().expect("启动 agent");
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("stderr 是 UTF-8");
    assert!(stderr.contains("workspace 路径无效"), "stderr: {stderr}");
}
