use std::path::PathBuf;

use runtime::logging::{LogConfig, with_logs};

fn log_file(directory: &std::path::Path) -> PathBuf {
    std::fs::read_dir(directory)
        .expect("读取日志目录")
        .map(|entry| entry.expect("读取日志项").path())
        .find(|path| {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            name.ends_with(".log")
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
    with_logs(&config, || tracing::info!(answer = 42, "日志管道自检")).expect("初始化日志");

    let path = log_file(&config.directory);
    let body = std::fs::read_to_string(&path).expect("读取日志文件");
    let line = body.lines().next().expect("至少一条日志");
    let record: serde_json::Value = serde_json::from_str(line).expect("每行是 JSON");
    assert_eq!(record["level"], "INFO");
    assert_eq!(record["message"], "日志管道自检");
    assert_eq!(record["answer"], 42);
    assert!(record["filename"].is_string(), "缺少来源文件: {record}");
}

/// 过滤器无效时回退到默认过滤器，并在标准错误给出警告，而不是阻断启动。
#[test]
fn falls_back_to_default_filter_when_filter_is_invalid() {
    let directory = tempfile::tempdir().expect("创建临时目录");
    let config = LogConfig {
        directory: directory.path().join("logs"),
        prefix: "hub".to_owned(),
        filter: "agent=notalevel".to_owned(),
    };
    with_logs(&config, || tracing::info!("无效过滤器已回退")).expect("无效过滤器不阻断启动");

    let body = std::fs::read_to_string(log_file(&config.directory)).expect("读取日志文件");
    assert!(
        body.contains("无效过滤器已回退"),
        "回退后仍写入日志: {body}"
    );
}
