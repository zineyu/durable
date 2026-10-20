use std::path::PathBuf;

use runtime::logging::{LogConfig, init};

fn log_file(directory: &std::path::Path) -> PathBuf {
    std::fs::read_dir(directory)
        .expect("读取日志目录")
        .map(|entry| entry.expect("读取日志项").path())
        .find(|path| {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            name.starts_with("agent.") && name.ends_with(".log")
        })
        .expect("日志文件已建立")
}

/// 日志文件每行一条 JSON 记录，含事件字段与来源位置。
#[test]
fn writes_json_lines_to_file() {
    let directory = tempfile::tempdir().expect("创建临时目录");
    let config = LogConfig {
        directory: directory.path().join("logs"),
        prefix: "agent".to_owned(),
        filter: "info".to_owned(),
    };
    let guard = init(&config).expect("初始化日志");
    tracing::info!(answer = 42, "日志管道自检");
    drop(guard);

    let path = log_file(&config.directory);
    let body = std::fs::read_to_string(&path).expect("读取日志文件");
    let line = body.lines().next().expect("至少一条日志");
    let record: serde_json::Value = serde_json::from_str(line).expect("每行是 JSON");
    assert_eq!(record["level"], "INFO");
    assert_eq!(record["message"], "日志管道自检");
    assert_eq!(record["answer"], 42);
    assert!(record["filename"].is_string(), "缺少来源文件: {record}");
}

/// 过滤器无效时返回显式错误，而不是静默丢弃全部日志。
#[test]
fn rejects_invalid_filter() {
    let directory = tempfile::tempdir().expect("创建临时目录");
    let config = LogConfig {
        directory: directory.path().join("logs"),
        prefix: "hub".to_owned(),
        filter: "agent=notalevel".to_owned(),
    };
    let error = init(&config).err().expect("无效过滤器必须失败");
    assert!(
        matches!(error, runtime::logging::LogError::Filter(_)),
        "错误类别: {error:?}"
    );
}
