use std::env::{VarError, var};
use std::fmt;

pub const MODEL_BASE_URL: &str = "DURABLE_MODEL_BASE_URL";
pub const MODEL_API_KEY: &str = "DURABLE_MODEL_API_KEY";
pub const MODEL_NAME: &str = "DURABLE_MODEL_NAME";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelConfig {
    pub base_url: String,
    pub api_key: String,
    pub model_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    Missing(Vec<&'static str>),
    NotUnicode(&'static str),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing(names) => write!(formatter, "缺少必需配置: {}", names.join(", ")),
            Self::NotUnicode(name) => write!(formatter, "配置不是 UTF-8: {name}"),
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

fn read_required(name: &'static str) -> Result<Option<String>, ConfigError> {
    match var(name) {
        Ok(value) if value.trim().is_empty() => Ok(None),
        Ok(value) => Ok(Some(value)),
        Err(VarError::NotPresent) => Ok(None),
        Err(VarError::NotUnicode(_)) => Err(ConfigError::NotUnicode(name)),
    }
}
