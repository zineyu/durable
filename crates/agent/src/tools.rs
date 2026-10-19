use async_trait::async_trait;
use durable::{ToolErrorCode, ToolOutcome, WorkspacePath};

use crate::cancel::Cancellation;

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

/// 执行工具调用。失败以结构化结果返回，不作为错误传播。
#[async_trait]
pub trait ToolBox: Send + Sync {
    async fn execute(&self, call: &ToolCall, context: &ToolContext) -> ToolOutcome;
}

/// 未注册任何工具的占位执行器：所有调用返回 `unknown_tool`。
pub struct NoTools;

#[async_trait]
impl ToolBox for NoTools {
    async fn execute(&self, call: &ToolCall, _context: &ToolContext) -> ToolOutcome {
        ToolOutcome::Failed {
            code: ToolErrorCode::UnknownTool,
            message: format!("未知工具: {}", call.name),
        }
    }
}
