use std::time::Duration;

use async_trait::async_trait;
use durable::{ToolErrorCode, ToolOutcome, WorkspacePath};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::cancel::Cancellation;

mod bash;
mod edit_file;
mod paths;
mod read_file;
mod write_file;

/// 一次工具调用的请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// 工具执行的上下文：workspace 边界与取消信号。
#[derive(Clone)]
pub struct ToolContext {
    pub workspace: WorkspacePath,
    pub cancellation: Cancellation,
}

/// 暴露给模型的工具定义，对应 `OpenAI` 的 `function` 声明。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolDefinition {
    pub name: &'static str,
    pub description: &'static str,
    pub parameters: Value,
}

impl ToolDefinition {
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({
            "type": "function",
            "function": {
                "name": self.name,
                "description": self.description,
                "parameters": self.parameters,
            },
        })
    }
}

/// 执行工具调用。失败以结构化结果返回，不作为错误传播。
#[async_trait]
pub trait ToolBox: Send + Sync {
    /// 每轮模型请求携带的工具定义。
    fn definitions(&self) -> Vec<ToolDefinition>;

    async fn execute(&self, call: &ToolCall, context: &ToolContext) -> ToolOutcome;
}

/// 未注册任何工具的占位执行器：所有调用返回 `unknown_tool`。
pub struct NoTools;

#[async_trait]
impl ToolBox for NoTools {
    fn definitions(&self) -> Vec<ToolDefinition> {
        Vec::new()
    }

    async fn execute(&self, call: &ToolCall, _context: &ToolContext) -> ToolOutcome {
        ToolOutcome::Failed {
            code: ToolErrorCode::UnknownTool,
            message: format!("未知工具: {}", call.name),
        }
    }
}

/// 工具的字节上限与超时。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolLimits {
    pub read_bytes: usize,
    pub bash_output_bytes: usize,
    pub bash_timeout: Duration,
}

pub(crate) const DEFAULT_READ_BYTES: usize = 256 * 1024;
pub(crate) const DEFAULT_BASH_OUTPUT_BYTES: usize = 64 * 1024;
pub(crate) const DEFAULT_BASH_TIMEOUT: Duration = Duration::from_secs(120);

impl Default for ToolLimits {
    fn default() -> Self {
        Self {
            read_bytes: DEFAULT_READ_BYTES,
            bash_output_bytes: DEFAULT_BASH_OUTPUT_BYTES,
            bash_timeout: DEFAULT_BASH_TIMEOUT,
        }
    }
}

/// 内置工具注册表：按名称分派到具体工具。
pub struct ToolRegistry {
    limits: ToolLimits,
}

impl ToolRegistry {
    #[must_use]
    pub fn new(limits: ToolLimits) -> Self {
        Self { limits }
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new(ToolLimits::default())
    }
}

#[async_trait]
impl ToolBox for ToolRegistry {
    fn definitions(&self) -> Vec<ToolDefinition> {
        vec![
            read_file::definition(),
            write_file::definition(),
            edit_file::definition(),
            bash::definition(),
        ]
    }

    async fn execute(&self, call: &ToolCall, context: &ToolContext) -> ToolOutcome {
        let result = match call.name.as_str() {
            "read_file" => read_file::read(call, context, self.limits.read_bytes),
            "write_file" => write_file::write(call, context),
            "edit_file" => edit_file::edit(call, context),
            "bash" => bash::run(call, context, &self.limits).await,
            _ => {
                return ToolOutcome::Failed {
                    code: ToolErrorCode::UnknownTool,
                    message: format!("未知工具: {}", call.name),
                };
            }
        };
        match result {
            Ok(output) => ToolOutcome::Succeeded { content: output },
            Err(failure) => failure.into(),
        }
    }
}

/// 工具失败的结构化原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ToolFailure {
    pub code: ToolErrorCode,
    pub message: String,
}

impl ToolFailure {
    pub(crate) fn new(code: ToolErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub(crate) fn io(action: &str, error: &std::io::Error) -> Self {
        Self::new(ToolErrorCode::IoError, format!("{action}: {error}"))
    }
}

impl From<ToolFailure> for ToolOutcome {
    fn from(failure: ToolFailure) -> Self {
        Self::Failed {
            code: failure.code,
            message: failure.message,
        }
    }
}

/// 解析工具参数。未知字段与缺失字段都是 `invalid_arguments`。
pub(crate) fn parse_arguments<T: DeserializeOwned>(call: &ToolCall) -> Result<T, ToolFailure> {
    serde_json::from_str(&call.arguments).map_err(|error| {
        ToolFailure::new(
            ToolErrorCode::InvalidArguments,
            format!("工具 {} 的参数无效: {error}", call.name),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_definitions_are_openai_functions() {
        for definition in ToolRegistry::default().definitions() {
            let value = definition.to_json();
            assert_eq!(value["type"], "function");
            assert_eq!(value["function"]["name"], definition.name);
            assert!(value["function"]["parameters"].is_object());
        }
    }

    #[test]
    fn unknown_fields_are_invalid_arguments() {
        let call = ToolCall {
            id: "call_1".into(),
            name: "read_file".into(),
            arguments: r#"{"path":"a.txt","encoding":"base64"}"#.into(),
        };
        let failure = parse_arguments::<read_file::Arguments>(&call).expect_err("未知字段被拒绝");
        assert_eq!(failure.code, ToolErrorCode::InvalidArguments);
    }
}
