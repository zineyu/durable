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
pub struct ConfigError {
    missing: Vec<&'static str>,
}

impl ConfigError {
    pub fn missing(&self) -> &[&'static str] {
        &self.missing
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "缺少必需配置: {}", self.missing.join(", "))
    }
}

impl std::error::Error for ConfigError {}

pub fn model_config_from_env() -> Result<ModelConfig, ConfigError> {
    let mut missing = Vec::new();
    let base_url = read(MODEL_BASE_URL, &mut missing);
    let api_key = read(MODEL_API_KEY, &mut missing);
    let model_name = read(MODEL_NAME, &mut missing);
    if !missing.is_empty() {
        return Err(ConfigError { missing });
    }
    Ok(ModelConfig {
        base_url: base_url.expect("已校验配置存在"),
        api_key: api_key.expect("已校验配置存在"),
        model_name: model_name.expect("已校验配置存在"),
    })
}

fn read(name: &'static str, missing: &mut Vec<&'static str>) -> Option<String> {
    match std::env::var(name) {
        Ok(value) if !value.trim().is_empty() => Some(value),
        _ => {
            missing.push(name);
            None
        }
    }
}
