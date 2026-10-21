use std::env::{VarError, var};
use std::fmt;
use std::time::Duration;

pub const MODEL_BASE_URL: &str = "DURABLE_MODEL_BASE_URL";
pub const MODEL_API_KEY: &str = "DURABLE_MODEL_API_KEY";
pub const MODEL_NAME: &str = "DURABLE_MODEL_NAME";
pub const CONTEXT_CHAR_LIMIT: &str = "DURABLE_CONTEXT_CHAR_LIMIT";
pub const MAX_STEPS: &str = "DURABLE_MAX_STEPS";
pub const MODEL_RETRY_LIMIT: &str = "DURABLE_MODEL_RETRY_LIMIT";
pub const MODEL_RETRY_BACKOFF_MS: &str = "DURABLE_MODEL_RETRY_BACKOFF_MS";

const DEFAULT_CONTEXT_CHAR_LIMIT: usize = 200_000;
const DEFAULT_MAX_STEPS: usize = 50;
const DEFAULT_MODEL_RETRY_LIMIT: usize = 3;
const DEFAULT_MODEL_RETRY_BACKOFF_MS: usize = 1_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelConfig {
    pub base_url: String,
    pub api_key: String,
    pub model_name: String,
}

/// run 引擎的边界配置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineConfig {
    pub context_char_limit: usize,
    pub max_steps: usize,
    /// 供应商限流时的重试次数上限。`0` 表示不重试。
    pub model_retry_limit: usize,
    /// 限流重试的退避底数，逐次翻倍。
    pub model_retry_backoff: Duration,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            context_char_limit: DEFAULT_CONTEXT_CHAR_LIMIT,
            max_steps: DEFAULT_MAX_STEPS,
            model_retry_limit: DEFAULT_MODEL_RETRY_LIMIT,
            model_retry_backoff: Duration::from_millis(DEFAULT_MODEL_RETRY_BACKOFF_MS as u64),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    Missing(Vec<&'static str>),
    NotUnicode(&'static str),
    InvalidNumber(&'static str, String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing(names) => write!(formatter, "缺少必需配置: {}", names.join(", ")),
            Self::NotUnicode(name) => write!(formatter, "配置不是 UTF-8: {name}"),
            Self::InvalidNumber(name, value) => {
                write!(formatter, "配置不是有效的正数: {name}={value}")
            }
        }
    }
}

impl std::error::Error for ConfigError {}

/// 从进程环境读取模型配置。
///
/// 失败：任一配置缺失或为空，或已存在但无法解析为 UTF-8。
pub fn model_config_from_env() -> Result<ModelConfig, ConfigError> {
    let base_url = read_required(MODEL_BASE_URL)?;
    let api_key = read_required(MODEL_API_KEY)?;
    let model_name = read_required(MODEL_NAME)?;
    let missing: Vec<&'static str> = [
        (MODEL_BASE_URL, base_url.is_none()),
        (MODEL_API_KEY, api_key.is_none()),
        (MODEL_NAME, model_name.is_none()),
    ]
    .into_iter()
    .filter_map(|(name, absent)| absent.then_some(name))
    .collect();
    match (base_url, api_key, model_name) {
        (Some(base_url), Some(api_key), Some(model_name)) => Ok(ModelConfig {
            base_url,
            api_key,
            model_name,
        }),
        _ => Err(ConfigError::Missing(missing)),
    }
}

/// 从进程环境读取 run 引擎配置。未设置时使用默认值。
///
/// 失败：已设置的配置无法解析为 UTF-8，或不是正整数（重试次数允许 `0`）。
pub fn engine_config_from_env() -> Result<EngineConfig, ConfigError> {
    let defaults = EngineConfig::default();
    Ok(EngineConfig {
        context_char_limit: read_positive(CONTEXT_CHAR_LIMIT, defaults.context_char_limit)?,
        max_steps: read_positive(MAX_STEPS, defaults.max_steps)?,
        model_retry_limit: read_count(MODEL_RETRY_LIMIT, defaults.model_retry_limit)?,
        model_retry_backoff: Duration::from_millis(read_positive(
            MODEL_RETRY_BACKOFF_MS,
            defaults.model_retry_backoff.as_millis() as usize,
        )? as u64),
    })
}

fn read_required(name: &'static str) -> Result<Option<String>, ConfigError> {
    match var(name) {
        Ok(value) if value.trim().is_empty() => Ok(None),
        Ok(value) => Ok(Some(value)),
        Err(VarError::NotPresent) => Ok(None),
        Err(VarError::NotUnicode(_)) => Err(ConfigError::NotUnicode(name)),
    }
}

fn read_positive(name: &'static str, default: usize) -> Result<usize, ConfigError> {
    let Some((value, raw)) = read_number(name)? else {
        return Ok(default);
    };
    if value == 0 {
        return Err(ConfigError::InvalidNumber(name, raw));
    }
    Ok(value)
}

/// 允许 `0` 的计数：用于「关掉」某项功能的重试次数。
fn read_count(name: &'static str, default: usize) -> Result<usize, ConfigError> {
    Ok(read_number(name)?.map_or(default, |(value, _)| value))
}

/// 读取整数配置。未设置时返回 `None`；错误说明里保留原文。
fn read_number(name: &'static str) -> Result<Option<(usize, String)>, ConfigError> {
    let Some(raw) = read_required(name)? else {
        return Ok(None);
    };
    match raw.parse() {
        Ok(value) => Ok(Some((value, raw))),
        Err(_) => Err(ConfigError::InvalidNumber(name, raw)),
    }
}
