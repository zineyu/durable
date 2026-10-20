use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// 未指定 `--database` 与 `--log-dir` 时，hub 的数据与日志落在标准目录。
#[test]
fn uses_standard_locations_when_paths_are_omitted() {
    let directory = tempfile::tempdir().expect("创建临时目录");
    let home = directory.path().join("home");
    std::fs::create_dir_all(&home).expect("创建 HOME");

    let mut child = Command::new(env!("CARGO_BIN_EXE_hub"))
        .args(["--listen", "127.0.0.1:0"])
        .env("HOME", &home)
        .env_remove("XDG_DATA_HOME")
        .env_remove("XDG_STATE_HOME")
        .env_remove("LOCALAPPDATA")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("启动 hub");

    let stdout = child.stdout.take().expect("取得启动输出");
    let mut line = String::new();
    BufReader::new(stdout)
        .read_line(&mut line)
        .expect("读取启动输出");
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

    let record = wait_for_startup_log(&dirs.logs);
    assert_eq!(record["database"], database.display().to_string());
    assert_eq!(record["log_directory"], dirs.logs.display().to_string());

    child.kill().expect("结束 hub");
    child.wait().expect("等待 hub 退出");
}

/// 在标准日志目录中等待带指定消息的 JSON 记录。
fn wait_for_startup_log(log_directory: &Path) -> serde_json::Value {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Some(record) = startup_log_record(log_directory, "hub 启动") {
            return record;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("未在 {} 找到启动日志", log_directory.display());
}

fn startup_log_record(log_directory: &Path, message: &str) -> Option<serde_json::Value> {
    for entry in std::fs::read_dir(log_directory).ok()?.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !(name.starts_with("hub.") && name.ends_with(".log")) {
            continue;
        }
        let body = std::fs::read_to_string(entry.path()).ok()?;
        for line in body.lines() {
            let Ok(record) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            if record["message"] == message {
                return Some(record);
            }
        }
    }
    None
}
