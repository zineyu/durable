//! 工具的 agent 侧：`ToolBox` 契约与按 host 路由到 link 的 `RemoteTools`。
//! 帧与子进程在 `super`；host 的 link 表在 `crate::hosts`；工具定义在 `durable::tool`，
//! 实现在 `apps/executor`。

use std::sync::Arc;

use async_trait::async_trait;
use durable::{Cancellation, HostName, ToolCall, ToolDefinition, ToolOutcome, WorkspacePath};

use super::LinkClosed;
use crate::hosts::HostRegistry;

/// 一次工具调用的终止方式。
///
/// 只有两支：得到工具结局，或宿主丢失。返回类型不是 `Result`，因此 `?` 无法把宿主丢失
/// 静默传给上一层；调用点必须显式处理两支。
#[derive(Debug, Clone)]
pub enum ToolEnd {
    /// 工具产生了结局。成功与失败都是结局。
    Ended(ToolOutcome),
    /// host 的 link 断开或答非所问；这次调用没有结局。
    HostGone(LinkClosed),
}

/// 工具执行的上下文：走哪台 host 的 link、workspace 边界与取消信号。
#[derive(Clone)]
pub struct ToolContext {
    /// session 所属 workspace 的 host。一次 run 的工具全部在这台 host 上执行。
    pub host: HostName,
    pub workspace: WorkspacePath,
    pub cancellation: Cancellation,
}

/// 执行工具调用。工具失败以结构化结局返回；宿主丢失是 [`ToolEnd`] 的第二支。
#[async_trait]
pub trait ToolBox: Send + Sync {
    /// 每轮模型请求携带的工具定义。
    fn definitions(&self) -> Vec<ToolDefinition>;

    async fn execute(&self, call: &ToolCall, context: &ToolContext) -> ToolEnd;
}

/// 把工具调用经 session 所属 host 的 link 交给 executor 的执行器。
///
/// 它不理解参数形状，也不重算路径边界：参数原样透传，判定全在 host 上。
pub struct RemoteTools {
    hosts: Arc<HostRegistry>,
}

impl RemoteTools {
    #[must_use]
    pub fn new(hosts: Arc<HostRegistry>) -> Self {
        Self { hosts }
    }
}

#[async_trait]
impl ToolBox for RemoteTools {
    fn definitions(&self) -> Vec<ToolDefinition> {
        durable::tool_definitions()
    }

    async fn execute(&self, call: &ToolCall, context: &ToolContext) -> ToolEnd {
        let Some(link) = self.hosts.link(&context.host).await else {
            return ToolEnd::HostGone(LinkClosed::unavailable(
                self.hosts.unavailable_detail(&context.host).await,
            ));
        };
        match link
            .request(
                context.workspace.as_str(),
                &call.name,
                &call.arguments,
                &context.cancellation,
            )
            .await
        {
            Ok(outcome) => ToolEnd::Ended(outcome),
            Err(closed) => ToolEnd::HostGone(closed),
        }
    }
}
