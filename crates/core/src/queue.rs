//! 待执行队列项：正文、锚点、待插入标记与创建时间；提交语义由 [`SubmitMode`] 表达。
//! 出队与执行在 `store/run`，自动标题在 `session`。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::id::{MessageId, QueueItemId};
use crate::message::MessageText;

/// 提交用户消息的语义。
///
/// 没有缺省值：提交请求必须写明是排队还是插话。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum SubmitMode {
    /// 等当前 run 结束后按 FIFO 执行。
    Queue,
    /// 在当前 run 的下一个步骤边界插入。
    Steer,
}

/// 等待执行的用户消息。session 级 FIFO，按 id 升序执行。
///
/// `anchor` 是本次执行接续的用户节点。`None` 表示 run 开始时在当前 active leaf 下新建用户节点；
/// `Some` 表示接续已存在的用户节点（编辑产生的新节点、重试 assistant 的父节点或待插入的插话）。
///
/// `pending_steer` 表示该项是等待插入当前 run 的插话。插话在提交时已经是一条用户节点；
/// run 结束前未被消费时降级为普通排队，节点由下一次 run 接续。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct QueueItem {
    #[ts(type = "number")]
    pub id: QueueItemId,
    pub text: MessageText,
    #[ts(type = "number | null")]
    pub anchor: Option<MessageId>,
    #[ts(type = "number")]
    pub created_at: i64,
    pub pending_steer: bool,
}
