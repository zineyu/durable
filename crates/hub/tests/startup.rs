#![expect(
    clippy::expect_used,
    clippy::panic,
    clippy::let_underscore_must_use,
    let_underscore_drop,
    reason = "测试脚手架用 expect 与 panic 直接报告夹具与断言失败；子进程读线程丢弃读取结果\
              与发送结果，因为主线程已按超时决定成败。"
)]

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// 未指定 `--database` 与 `--log-dir` 时，hub 的数据与日志落在标准目录。
///
/// 用例清除 `runtime::dirs::VARIABLES` 中的全部变量后只设置 `HOME`：三个平台在各自的
/// 原生变量缺失时都回退到 `HOME`，因此期望值可从同一份 `Env` 推导。
#[tokio::test]
async fn uses_standard_locations_when_paths_are_omitted() {
    let directory = tempfile::tempdir().expect("创建临时目录");
    let home = directory.path().join("home");
    std::fs::create_dir_all(&home).expect("创建 HOME");

    let mut command = Command::new(env!("CARGO_BIN_EXE_hub"));
    command
        .args(["--listen", "127.0.0.1:0"])
        .env("HOME", &home)
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    for name in runtime::dirs::VARIABLES {
        if name != "HOME" {
            command.env_remove(name);
        }
    }
    let mut child = command.spawn().expect("启动 hub");

    let line = first_line(&mut child);
    assert!(
        line.starts_with("hub listening on http://"),
        "启动输出: {line}"
    );

    let dirs = runtime::dirs::resolve(&runtime::dirs::Env {
        home: Some(home),
        ..runtime::dirs::Env::default()
    })
    .expect("解析标准目录");
    let database = dirs.data.join(hub::DEFAULT_DATABASE_FILE);
    assert!(
        database.is_file(),
        "默认数据库未建立: {}",
        database.display()
    );

    let record = wait_for_startup_log(&dirs.logs).await;
    assert_eq!(record["database"], database.display().to_string());
    assert_eq!(record["log_directory"], dirs.logs.display().to_string());

    child.kill().expect("结束 hub");
    child.wait().expect("等待 hub 退出");
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
        if let Some(record) = startup_log_record(log_directory, "hub 启动") {
            return record;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("未在 {} 找到启动日志", log_directory.display());
}

fn startup_log_record(log_directory: &Path, message: &str) -> Option<serde_json::Value> {
    for entry in std::fs::read_dir(log_directory).ok()?.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let is_log = Path::new(name)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("log"));
        if !(name.starts_with("hub.") && is_log) {
            continue;
        }
        let body = std::fs::read_to_string(entry.path()).ok()?;
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
