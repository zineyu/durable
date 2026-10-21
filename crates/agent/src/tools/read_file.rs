use std::fmt::Write;

use durable::ToolErrorCode;
use serde::Deserialize;
use serde_json::json;

use super::paths::Workspace;
use super::{ToolCall, ToolContext, ToolDefinition, ToolFailure, parse_arguments};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Arguments {
    pub path: String,
    #[serde(default)]
    pub start_line: Option<usize>,
    #[serde(default)]
    pub end_line: Option<usize>,
}

pub(crate) fn definition() -> ToolDefinition {
    ToolDefinition {
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
    }
}

/// 读取文件并按行编号输出。
///
/// 失败：路径越界、文件不存在、非 UTF-8、超过字节上限、行区间非法。
pub(crate) fn read(
    call: &ToolCall,
    context: &ToolContext,
    limit: usize,
) -> Result<String, ToolFailure> {
    let arguments: Arguments = parse_arguments(call)?;
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
    if metadata.len() > limit as u64 {
        return Err(limit_failure(limit, metadata.len()));
    }
    let bytes = std::fs::read(&path).map_err(|error| ToolFailure::io("读取文件", &error))?;
    if bytes.len() > limit {
        return Err(limit_failure(limit, bytes.len() as u64));
    }
    let text = String::from_utf8(bytes).map_err(|_| {
        ToolFailure::new(
            ToolErrorCode::NotUtf8,
            format!("文件不是 UTF-8: {}", arguments.path),
        )
    })?;
    let lines: Vec<&str> = text.lines().collect();
    let (start, end) = select_range(&arguments, lines.len())?;
    if start > end {
        return Ok(String::new());
    }
    let mut output = String::new();
    for (offset, line) in lines
        .iter()
        .enumerate()
        .skip(start - 1)
        .take(end - start + 1)
    {
        writeln!(output, "{}\t{line}", offset + 1).map_err(|error| {
            ToolFailure::new(ToolErrorCode::IoError, format!("写入读取结果: {error}"))
        })?;
    }
    Ok(output)
}

fn limit_failure(limit: usize, size: u64) -> ToolFailure {
    ToolFailure::new(
        ToolErrorCode::ReadLimitExceeded,
        format!("文件 {size} 字节，超过 {limit} 字节读取上限"),
    )
}

/// 解析 1 起的闭区间行号。
///
/// 失败：起始行为 0、结束行早于起始行，或区间超出文件行数。
fn select_range(arguments: &Arguments, line_count: usize) -> Result<(usize, usize), ToolFailure> {
    match (arguments.start_line, arguments.end_line) {
        (None, None) => Ok((1, line_count)),
        (start_line, end_line) => {
            let start = start_line.unwrap_or(1);
            let end = end_line.unwrap_or(line_count);
            if start < 1 || end < start || end > line_count {
                return Err(ToolFailure::new(
                    ToolErrorCode::InvalidLineRange,
                    format!("行区间非法: {start}..{end}，文件共 {line_count} 行"),
                ));
            }
            Ok((start, end))
        }
    }
}
