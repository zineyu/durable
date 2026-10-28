//! 工具契约：定义、字节上限、失败原因，以及工具调用的结局与错误码。
//! 工具实现在 `crates/executor/src/tools`；agent 只按定义拼模型请求并透传参数。

use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use ts_rs::TS;

/// 工具失败的稳定类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ToolErrorCode {
    UnknownTool,
    InvalidArguments,
    PathEscape,
    FileNotFound,
    NotUtf8,
    ReadLimitExceeded,
    InvalidLineRange,
    ParentNotFound,
    EditNotFound,
    EditNotUnique,
    BashTimeout,
    IoError,
    Cancelled,
}

/// 工具执行返回的结局。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "status",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ToolOutcome {
    Succeeded {
        content: String,
    },
    Failed {
        code: ToolErrorCode,
        message: String,
    },
}

/// tool 节点里工具结果块的状态。
///
/// 工具在执行前入树，此时为 [`ToolResultState::Running`]；执行完毕写入结局；
/// 非幂等工具在执行中崩溃后写 [`ToolResultState::Unknown`]，表示丢失了观测。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "status",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ToolResultState {
    Running,
    Finished { outcome: ToolOutcome },
    Unknown,
}

impl From<ToolOutcome> for ToolResultState {
    fn from(outcome: ToolOutcome) -> Self {
        Self::Finished { outcome }
    }
}

/// 暴露给模型的工具定义，对应 `OpenAI` 的 `function` 声明。
///
/// 定义只有一份：agent 用它拼模型请求，executor 用同一份名称校验参数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolDefinition {
    pub name: &'static str,
    pub description: &'static str,
    pub parameters: Value,
    /// 重复执行给出相同结果且无额外副作用。只有幂等工具允许在崩溃后重跑。
    pub idempotent: bool,
}

impl ToolDefinition {
    /// 转成模型请求里的 `function` 声明。
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

/// 四个工具的完整定义表，顺序即模型请求里的声明顺序。
#[must_use]
pub fn tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            idempotent: true,
            name: "read_file",
            description: "读取 workspace 根内的 UTF-8 文本文件。可选起止行（1 起，含两端）。输出每行带行号。",
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "相对 workspace 根的路径" },
                    "start_line": { "type": "integer", "minimum": 1, "description": "起始行号，含" },
                    "end_line": { "type": "integer", "minimum": 1, "description": "结束行号，含" },
                },
                "required": ["path"],
                "additionalProperties": false,
            }),
        },
        ToolDefinition {
            idempotent: true,
            name: "write_file",
            description: "用给定内容覆盖写入 workspace 根内的文件。父目录必须已存在，不隐式创建目录。",
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "相对 workspace 根的路径" },
                    "content": { "type": "string", "description": "文件完整内容" },
                },
                "required": ["path", "content"],
                "additionalProperties": false,
            }),
        },
        ToolDefinition {
            idempotent: false,
            name: "edit_file",
            description: "把 workspace 根内文件中唯一匹配的 old_string 替换为 new_string。不做正则与全部替换。",
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "相对 workspace 根的路径" },
                    "old_string": { "type": "string", "description": "必须恰好匹配一次的原文" },
                    "new_string": { "type": "string", "description": "替换后的文本" },
                },
                "required": ["path", "old_string", "new_string"],
                "additionalProperties": false,
            }),
        },
        ToolDefinition {
            idempotent: false,
            name: "bash",
            description: "在 workspace 根内以 sh -c 执行命令。stdin 关闭，stdout 与 stderr 合并返回，返回退出码与耗时。",
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "交给 sh -c 的命令" },
                    "cwd": { "type": "string", "description": "相对 workspace 根的工作目录，默认根" },
                },
                "required": ["command"],
                "additionalProperties": false,
            }),
        },
    ]
}

/// 按名称取单个工具定义；未登记的工具返回 `None`。
#[must_use]
pub fn tool_definition(name: &str) -> Option<ToolDefinition> {
    tool_definitions()
        .into_iter()
        .find(|definition| definition.name == name)
}

/// 工具的字节上限与超时。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolLimits {
    pub read_bytes: usize,
    pub bash_output_bytes: usize,
    pub bash_timeout: Duration,
}

pub const DEFAULT_READ_BYTES: usize = 256 * 1024;
pub const DEFAULT_BASH_OUTPUT_BYTES: usize = 64 * 1024;
pub const DEFAULT_BASH_TIMEOUT: Duration = Duration::from_secs(120);

impl Default for ToolLimits {
    fn default() -> Self {
        Self {
            read_bytes: DEFAULT_READ_BYTES,
            bash_output_bytes: DEFAULT_BASH_OUTPUT_BYTES,
            bash_timeout: DEFAULT_BASH_TIMEOUT,
        }
    }
}

/// 工具失败的结构化原因。失败回传模型，run 继续。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolFailure {
    pub code: ToolErrorCode,
    pub message: String,
}

impl ToolFailure {
    #[must_use]
    pub fn new(code: ToolErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn io(action: &str, error: &std::io::Error) -> Self {
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
///
/// # Errors
///
/// 参数不是合法 JSON、含未知字段或缺少必填字段时返回 [`ToolErrorCode::InvalidArguments`]。
pub fn parse_arguments<T: DeserializeOwned>(tool: &str, arguments: &str) -> Result<T, ToolFailure> {
    serde_json::from_str(arguments).map_err(|error| {
        ToolFailure::new(
            ToolErrorCode::InvalidArguments,
            format!("工具 {tool} 的参数无效: {error}"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_definitions_are_openai_functions() {
        for definition in tool_definitions() {
            let value = definition.to_json();
            assert_eq!(value["type"], "function");
            assert_eq!(value["function"]["name"], definition.name);
            assert!(value["function"]["parameters"].is_object());
        }
    }

    #[test]
    fn every_definition_is_reachable_by_name() {
        for definition in tool_definitions() {
            let found = tool_definition(definition.name).expect("定义可按名称取回");
            assert_eq!(found, definition);
        }
        assert!(tool_definition("move_file").is_none());
    }

    #[test]
    fn unknown_fields_are_invalid_arguments() {
        #[derive(Debug, serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Arguments {
            path: String,
        }
        let arguments: Arguments =
            parse_arguments("read_file", r#"{"path":"a"}"#).expect("合法参数");
        assert_eq!(arguments.path, "a");
        let failure =
            parse_arguments::<Arguments>("read_file", r#"{"path":"a","encoding":"base64"}"#)
                .expect_err("未知字段被拒绝");
        assert_eq!(failure.code, ToolErrorCode::InvalidArguments);
        assert!(failure.message.starts_with("工具 read_file 的参数无效"));
    }
}
