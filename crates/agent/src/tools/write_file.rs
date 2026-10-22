//! `write_file` 工具：覆盖写入；父目录不存在即报错。

use serde::Deserialize;
use serde_json::json;

use super::paths::Workspace;
use super::{ToolCall, ToolContext, ToolDefinition, ToolFailure, parse_arguments};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Arguments {
    pub path: String,
    pub content: String,
}

pub(crate) fn definition() -> ToolDefinition {
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
    }
}

/// 覆盖写入文件。
///
/// 失败：路径越界、父目录不存在、符号链接目标非法、无法写入。
pub(crate) fn write(call: &ToolCall, context: &ToolContext) -> Result<String, ToolFailure> {
    let arguments: Arguments = parse_arguments(call)?;
    let workspace = Workspace::new(context.workspace.as_str());
    let path = workspace.resolve_for_write(&arguments.path)?;
    std::fs::write(&path, &arguments.content)
        .map_err(|error| ToolFailure::io("写入文件", &error))?;
    Ok(format!(
        "写入 {} 字节: {}",
        arguments.content.len(),
        arguments.path
    ))
}
