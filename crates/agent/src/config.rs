use std::env::{VarError, var};
use std::fmt;

pub const MODEL_BASE_URL: &str = "DURABLE_MODEL_BASE_URL";
pub const MODEL_API_KEY: &str = "DURABLE_MODEL_API_KEY";
pub const MODEL_NAME: &str = "DURABLE_MODEL_NAME";
pub const MAX_CONCURRENCY: &str = "DURABLE_MAX_CONCURRENCY";
pub const CONTEXT_CHAR_LIMIT: &str = "DURABLE_CONTEXT_CHAR_LIMIT";
pub const MAX_STEPS: &str = "DURABLE_MAX_STEPS";

const DEFAULT_MAX_CONCURRENCY: usize = 4;
const DEFAULT_CONTEXT_CHAR_LIMIT: usize = 200_000;
const DEFAULT_MAX_STEPS: usize = 50;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelConfig {
    pub base_url: String,
    pub api_key: String,
    pub model_name: String,
}

/// run 引擎的调度与边界配置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineConfig {
    pub max_concurrency: usize,
    pub context_char_limit: usize,
    pub max_steps: usize,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            max_concurrency: DEFAULT_MAX_CONCURRENCY,
            context_char_limit: DEFAULT_CONTEXT_CHAR_LIMIT,
            max_steps: DEFAULT_MAX_STEPS,
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
/// 失败：已设置的配置无法解析为 UTF-8，或不是正整数。
pub fn engine_config_from_env() -> Result<EngineConfig, ConfigError> {
    let defaults = EngineConfig::default();
    Ok(EngineConfig {
        max_concurrency: read_positive(MAX_CONCURRENCY, defaults.max_concurrency)?,
        context_char_limit: read_positive(CONTEXT_CHAR_LIMIT, defaults.context_char_limit)?,
        max_steps: read_positive(MAX_STEPS, defaults.max_steps)?,
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
    let Some(raw) = read_required(name)? else {
        return Ok(default);
    };
    let value: usize = raw
        .parse()
        .map_err(|_| ConfigError::InvalidNumber(name, raw.clone()))?;
    if value == 0 {
        return Err(ConfigError::InvalidNumber(name, raw));
    }
    Ok(value)
}
