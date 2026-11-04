//! 四个工具与统一入口：按名称分派，参数校验与路径边界都在本 crate。
//! 工具定义在 `durable::tool`，帧的读写与请求生命周期在 `main`。
//!
//! 规格 user story R-28、R-29、R-30、R-31、R-32、R-33：文件与命令都在 host 上执行，
//! 路径逃逸在 host 上被拒绝，失败的结局照旧回传，取消时进程组被杀掉。

use durable::{Cancellation, ToolErrorCode, ToolFailure, ToolLimits, ToolOutcome};

mod bash;
mod edit_file;
mod paths;
mod read_file;
mod write_file;

pub(crate) use paths::check_workspace;

/// 执行一次工具调用。失败以结构化结果返回，不作为错误传播。
pub(crate) async fn execute(
    tool: &str,
    workspace: &str,
    arguments: &str,
    cancellation: &Cancellation,
    limits: &ToolLimits,
) -> ToolOutcome {
    let result: Result<String, ToolFailure> = match tool {
        "read_file" => read_file::read(workspace, arguments, limits.read_bytes),
        "write_file" => write_file::write(workspace, arguments),
        "edit_file" => edit_file::edit(workspace, arguments),
        "bash" => bash::run(workspace, arguments, cancellation, limits).await,
        _ => {
            return ToolOutcome::Failed {
                code: ToolErrorCode::UnknownTool,
                message: format!("未知工具: {tool}"),
            };
        }
    };
    match result {
        Ok(content) => ToolOutcome::Succeeded { content },
        Err(failure) => failure.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn unknown_tool_is_reported_as_a_tool_failure() {
        let outcome = execute(
            "move_file",
            "/",
            "{}",
            &Cancellation::new(),
            &ToolLimits::default(),
        )
        .await;
        assert_eq!(
            outcome,
            ToolOutcome::Failed {
                code: ToolErrorCode::UnknownTool,
                message: "未知工具: move_file".into(),
            }
        );
    }

    #[tokio::test]
    async fn invalid_arguments_are_reported_before_touching_the_filesystem() {
        let outcome = execute(
            "read_file",
            "/",
            r#"{"path":1}"#,
            &Cancellation::new(),
            &ToolLimits::default(),
        )
        .await;
        assert!(
            matches!(
                outcome,
                ToolOutcome::Failed {
                    code: ToolErrorCode::InvalidArguments,
                    ..
                }
            ),
            "得到 {outcome:?}"
        );
    }
}
