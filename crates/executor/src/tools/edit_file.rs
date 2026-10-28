//! `edit_file` 工具：唯一匹配替换；未找到与多处匹配是两种错误。
//! 工具定义在 `durable::tool`，路径边界在 `paths`。

use durable::{ToolErrorCode, ToolFailure, parse_arguments};
use serde::Deserialize;

use super::paths::Workspace;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Arguments {
    pub path: String,
    pub old_string: String,
    pub new_string: String,
}

/// 替换唯一匹配的原文。
///
/// 失败：路径越界、文件不存在、非 UTF-8、匹配零次或多于一次、无法写入。
pub(crate) fn edit(workspace: &str, arguments: &str) -> Result<String, ToolFailure> {
    let arguments: Arguments = parse_arguments("edit_file", arguments)?;
    if arguments.old_string.is_empty() {
        return Err(ToolFailure::new(
            ToolErrorCode::InvalidArguments,
            "old_string 不能为空",
        ));
    }
    let workspace = Workspace::new(workspace);
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
