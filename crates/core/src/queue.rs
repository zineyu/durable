use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::id::{MessageId, QueueItemId};
use crate::message::MessageText;

/// 等待执行的用户消息。session 级 FIFO，按 id 升序执行。
///
/// `anchor` 是本次执行接续的用户节点。`None` 表示 run 开始时在当前 active leaf 下新建用户节点；
/// `Some` 表示接续已存在的用户节点（编辑产生的新节点或重试 assistant 的父节点）。
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
}
