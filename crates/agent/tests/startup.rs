#![expect(
    clippy::expect_used,
    clippy::panic,
    clippy::let_underscore_must_use,
    let_underscore_drop,
    reason = "进程级测试脚本用 panic 与 expect 直接报告夹具与断言失败。"
)]

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

mod support;

fn run_agent(directory: &Path, configure: impl FnOnce(&mut Command)) -> std::process::Output {
    let mut command = support::command(&directory.join("agent.sqlite"));
    configure(&mut command);
    command.output().expect("启动 agent 二进制")
}

async fn spawn_agent(directory: &Path, workspaces: &[PathBuf]) -> (tokio::process::Child, String) {
    let mut command = support::serving_command(&directory.join("agent.sqlite"));
    for workspace in workspaces {
        command
            .arg("--workspace")
            .arg(format!("local:{}", workspace.display()));
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
        command.arg("--workspace").arg(format!(
            "local:{}",
            directory.path().join("missing").display()
        ));
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
    assert!(
        after.max_event_id >= before.max_event_id,
        "重复传入已注册的初始 workspace 不写 workspace 事件；link 连接过程中的 host 状态事实会推进事件 id"
    );
    second.start_kill().expect("结束第二个 agent");
    second.wait().await.expect("等待第二个 agent 退出");
}
/// 规格「进程与部署」：未指定数据库与日志目录时使用平台标准目录。
///
/// 用例清除 `runtime::dirs::VARIABLES` 中的全部变量后只设置 `HOME`：三个平台在各自的
/// 原生变量缺失时都回退到 `HOME`，因此期望值可从同一份 `Env` 推导。
#[tokio::test]
async fn uses_standard_locations_when_paths_are_omitted() {
    let directory = tempfile::tempdir().expect("创建临时目录");
    let home = directory.path().join("home");
    std::fs::create_dir_all(&home).expect("创建 HOME");

    let mut command = support::command_without_paths();
    command
        .args(["--listen", "127.0.0.1:0"])
        .env("HOME", &home)
        .env(runtime::logging::FILTER_ENV, "debug")
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    for name in runtime::dirs::VARIABLES {
        if name != "HOME" {
            command.env_remove(name);
        }
    }
    let mut child = command.spawn().expect("启动 agent");
    let line = first_line(&mut child);
    assert!(
        line.starts_with("agent listening on http://"),
        "启动输出: {line}"
    );

    let dirs = runtime::dirs::resolve(&runtime::dirs::Env {
        home: Some(home),
        ..runtime::dirs::Env::default()
    })
    .expect("解析标准目录");
    let database = dirs.data.join(agent::DEFAULT_DATABASE_FILE);
    assert!(
        database.is_file(),
        "默认数据库未建立: {}",
        database.display()
    );

    let record = wait_for_startup_log(&dirs.logs).await;
    assert_eq!(record["database"], database.display().to_string());
    assert_eq!(record["log_directory"], dirs.logs.display().to_string());
    assert!(
        startup_log_record(&dirs.logs, "打开 SQLite").is_some(),
        "内核的 `tracing` 事件进入同一订阅者"
    );

    child.kill().expect("结束 agent");
    child.wait().expect("等待 agent 退出");
}

/// 读取启动输出的首行。超时后结束子进程，避免用例挂起。
fn first_line(child: &mut std::process::Child) -> String {
    let stdout = child.stdout.take().expect("取得启动输出");
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let _ = BufReader::new(stdout).read_line(&mut line);
        let _ = sender.send(line);
    });
    match receiver.recv_timeout(Duration::from_secs(10)) {
        Ok(line) => line.trim_end().to_owned(),
        Err(error) => {
            child.kill().expect("结束未就绪的进程");
            panic!("未在 10 秒内读到启动输出: {error}");
        }
    }
}

/// 在标准日志目录中等待带指定消息的 JSON 记录。
async fn wait_for_startup_log(log_directory: &Path) -> serde_json::Value {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Some(record) = startup_log_record(log_directory, "agent 启动") {
            return record;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("未在 {} 找到启动日志", log_directory.display());
}

fn startup_log_record(log_directory: &Path, message: &str) -> Option<serde_json::Value> {
    for entry in std::fs::read_dir(log_directory).ok()?.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.starts_with("agent.") {
            continue;
        }
        let Some(extension) = path.extension() else {
            continue;
        };
        if !extension.eq_ignore_ascii_case("log") {
            continue;
        }
        let body = std::fs::read_to_string(&path).ok()?;
        for line in body.lines() {
            let Ok(record) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            if record.get("message").and_then(serde_json::Value::as_str) == Some(message) {
                return Some(record);
            }
        }
    }
    None
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
