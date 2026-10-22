//! `edit_file` 工具：唯一匹配替换；未找到与多处匹配是两种错误。

use durable::ToolErrorCode;
use serde::Deserialize;
use serde_json::json;

use super::paths::Workspace;
use super::{ToolCall, ToolContext, ToolDefinition, ToolFailure, parse_arguments};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Arguments {
    pub path: String,
    pub old_string: String,
    pub new_string: String,
}

pub(crate) fn definition() -> ToolDefinition {
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
    }
}

/// 替换唯一匹配的原文。
///
/// 失败：路径越界、文件不存在、非 UTF-8、匹配零次或多于一次、无法写入。
pub(crate) fn edit(call: &ToolCall, context: &ToolContext) -> Result<String, ToolFailure> {
    let arguments: Arguments = parse_arguments(call)?;
    if arguments.old_string.is_empty() {
        return Err(ToolFailure::new(
            ToolErrorCode::InvalidArguments,
            "old_string 不能为空",
        ));
    }
    let workspace = Workspace::new(context.workspace.as_str());
    let path = workspace.resolve_existing(&arguments.path)?;
    let metadata =
        std::fs::metadata(&path).map_err(|error| ToolFailure::io("读取文件属性", &error))?;
    if !metadata.is_file() {
        return Err(ToolFailure::new(
            ToolErrorCode::IoError,
            format!("路径不是普通文件: {}", arguments.path),
        ));
    }
    let bytes = std::fs::read(&path).map_err(|error| ToolFailure::io("读取文件", &error))?;
    let text = String::from_utf8(bytes).map_err(|_| {
        ToolFailure::new(
            ToolErrorCode::NotUtf8,
            format!("文件不是 UTF-8: {}", arguments.path),
        )
    })?;
    match text.matches(&arguments.old_string).count() {
        0 => {
            return Err(ToolFailure::new(
                ToolErrorCode::EditNotFound,
                format!("old_string 未匹配: {}", arguments.path),
            ));
        }
        1 => {}
        count => {
            return Err(ToolFailure::new(
                ToolErrorCode::EditNotUnique,
                format!("old_string 匹配 {count} 处，必须唯一: {}", arguments.path),
            ));
        }
    }
    let updated = text.replacen(&arguments.old_string, &arguments.new_string, 1);
    std::fs::write(&path, &updated).map_err(|error| ToolFailure::io("写入文件", &error))?;
    Ok(format!(
        "替换 1 处，写入 {} 字节: {}",
        updated.len(),
        arguments.path
    ))
}
