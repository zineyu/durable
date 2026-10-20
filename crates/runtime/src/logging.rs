//! 日志初始化：标准错误与每日轮转文件双写。

use std::env;
use std::fs;
use std::io::{self, IsTerminal};
use std::path::PathBuf;

use thiserror::Error;
use tracing_appender::non_blocking::{NonBlocking, NonBlockingBuilder, WorkerGuard};
use tracing_appender::rolling::{InitError, RollingFileAppender, Rotation};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt;

/// 日志过滤器的环境变量。取值同 `RUST_LOG` 指令。
pub const FILTER_ENV: &str = "DURABLE_LOG";
/// 未设置过滤器时的默认值。
pub const DEFAULT_FILTER: &str = "info";
/// 日志文件的保留个数（每天一个文件）。
pub const MAX_LOG_FILES: usize = 14;

/// 日志初始化参数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogConfig {
    pub directory: PathBuf,
    pub prefix: String,
    pub filter: String,
}

impl LogConfig {
    /// 用给定目录与前缀构造参数，过滤器取自 [`FILTER_ENV`]。
    pub fn new(directory: impl Into<PathBuf>, prefix: impl Into<String>) -> Self {
        Self {
            directory: directory.into(),
            prefix: prefix.into(),
            filter: filter_from_env(),
        }
    }
}

/// 读取日志过滤器；未设置或为空白时使用 [`DEFAULT_FILTER`]。
pub fn filter_from_env() -> String {
    filter_or_default(env::var(FILTER_ENV).ok())
}

fn filter_or_default(value: Option<String>) -> String {
    value
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_FILTER.to_owned())
}

#[derive(Debug, Error)]
pub enum LogError {
    #[error("无法建立日志目录 {path}: {source}")]
    Directory {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("无法建立日志文件 {path}: {source}")]
    File {
        path: PathBuf,
        #[source]
        source: InitError,
    },
    #[error("日志过滤器无效: {0}")]
    Filter(String),
    #[error("日志订阅者无法安装: {0}")]
    Install(String),
}

/// 日志句柄。进程退出前保持存活，后台写入线程随它刷新并结束。
pub struct LogGuard {
    _worker: WorkerGuard,
}

/// 同时向标准错误与每日轮转文件写入日志。
///
/// 标准错误用适合阅读的多行格式；文件每行一条 JSON 记录，便于检索与聚合。
/// 文件名为 `<prefix>.<YYYY-MM-DD>.log`，只保留最近 [`MAX_LOG_FILES`] 个。
/// 标准错误在终端中带颜色，重定向时不带颜色。
///
/// 失败：日志目录无法建立、日志文件无法建立、过滤器无效或全局订阅者已安装。
pub fn init(config: &LogConfig) -> Result<LogGuard, LogError> {
    fs::create_dir_all(&config.directory).map_err(|source| LogError::Directory {
        path: config.directory.clone(),
        source,
    })?;
    let appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(&config.prefix)
        .filename_suffix("log")
        .max_log_files(MAX_LOG_FILES)
        .build(&config.directory)
        .map_err(|source| LogError::File {
            path: config.directory.clone(),
            source,
        })?;
    let (writer, worker) = NonBlockingBuilder::default().lossy(true).finish(appender);
    let filter = EnvFilter::try_new(&config.filter)
        .map_err(|error| LogError::Filter(format!("{}: {error}", config.filter)))?;
    tracing_subscriber::registry()
        .with(filter)
        .with(console_layer())
        .with(file_layer(writer))
        .try_init()
        .map_err(|error| LogError::Install(error.to_string()))?;
    Ok(LogGuard { _worker: worker })
}

fn console_layer<S>() -> impl Layer<S>
where
    S: tracing::Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    tracing_subscriber::fmt::layer()
        .pretty()
        .with_writer(io::stderr)
        .with_ansi(io::stderr().is_terminal())
}

fn file_layer<S>(writer: NonBlocking) -> impl Layer<S>
where
    S: tracing::Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    tracing_subscriber::fmt::layer()
        .json()
        .flatten_event(true)
        .with_writer(writer)
        .with_ansi(false)
        .with_file(true)
        .with_line_number(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_defaults_when_absent_or_blank() {
        assert_eq!(filter_or_default(None), DEFAULT_FILTER);
        assert_eq!(filter_or_default(Some(String::new())), DEFAULT_FILTER);
        assert_eq!(filter_or_default(Some("  ".to_owned())), DEFAULT_FILTER);
    }

    #[test]
    fn filter_keeps_explicit_directives() {
        assert_eq!(
            filter_or_default(Some("agent=debug".to_owned())),
            "agent=debug"
        );
    }
}
