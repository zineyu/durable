//! 工具的 agent 侧：`ToolBox` 契约与经 link 执行工具的 `RemoteTools`。
//! 帧与子进程在 `super`；工具定义在 `durable::tool`，实现在 `crates/executor`。

use async_trait::async_trait;
use durable::{Cancellation, ToolDefinition, ToolErrorCode, ToolOutcome, WorkspacePath};

use super::{LinkClosed, LinkHandle};

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

/// 执行工具调用。工具失败以结构化结局返回；link 断开是运维失败，作为错误传播。
#[async_trait]
pub trait ToolBox: Send + Sync {
    /// 每轮模型请求携带的工具定义。
    fn definitions(&self) -> Vec<ToolDefinition>;

    async fn execute(
        &self,
        call: &ToolCall,
        context: &ToolContext,
    ) -> Result<ToolOutcome, LinkClosed>;

    /// 该工具是否幂等。未登记的工具按非幂等处理。
    fn idempotent(&self, name: &str) -> bool {
        self.definitions()
            .iter()
            .any(|definition| definition.name == name && definition.idempotent)
    }
}

/// 未注册任何工具的占位执行器：所有调用返回 `unknown_tool`。
pub struct NoTools;

#[async_trait]
impl ToolBox for NoTools {
    fn definitions(&self) -> Vec<ToolDefinition> {
        Vec::new()
    }

    async fn execute(
        &self,
        call: &ToolCall,
        _context: &ToolContext,
    ) -> Result<ToolOutcome, LinkClosed> {
        Ok(ToolOutcome::Failed {
            code: ToolErrorCode::UnknownTool,
            message: format!("未知工具: {}", call.name),
        })
    }
}

/// 把工具调用经 link 交给 executor 的执行器。它不理解参数形状。
pub struct RemoteTools {
    link: LinkHandle,
}

impl RemoteTools {
    #[must_use]
    pub fn new(link: LinkHandle) -> Self {
        Self { link }
    }
}

#[async_trait]
impl ToolBox for RemoteTools {
    fn definitions(&self) -> Vec<ToolDefinition> {
        durable::tool_definitions()
    }

    async fn execute(
        &self,
        call: &ToolCall,
        context: &ToolContext,
    ) -> Result<ToolOutcome, LinkClosed> {
        self.link
            .request(
                context.workspace.as_str(),
                &call.name,
                &call.arguments,
                &context.cancellation,
            )
            .await
    }

    fn idempotent(&self, name: &str) -> bool {
        durable::tool_definition(name).is_some_and(|definition| definition.idempotent)
    }
}
