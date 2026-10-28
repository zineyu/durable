//! `write_file` 工具：覆盖写入；父目录不存在即报错。
//! 工具定义在 `durable::tool`，路径边界在 `paths`。

use durable::{ToolFailure, parse_arguments};
use serde::Deserialize;

use super::paths::Workspace;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Arguments {
    pub path: String,
    pub content: String,
}

/// 覆盖写入文件。
///
/// 失败：路径越界、父目录不存在、符号链接目标非法、无法写入。
pub(crate) fn write(workspace: &str, arguments: &str) -> Result<String, ToolFailure> {
    let arguments: Arguments = parse_arguments("write_file", arguments)?;
    let workspace = Workspace::new(workspace);
    let path = workspace.resolve_for_write(&arguments.path)?;
    std::fs::write(&path, &arguments.content)
        .map_err(|error| ToolFailure::io("写入文件", &error))?;
    Ok(format!(
        "写入 {} 字节: {}",
        arguments.content.len(),
        arguments.path
    ))
}
