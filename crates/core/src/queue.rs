use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::id::QueueItemId;
use crate::message::MessageText;

/// 等待执行的用户消息。session 级 FIFO，按 id 升序执行。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct QueueItem {
    #[ts(type = "number")]
    pub id: QueueItemId,
    pub text: MessageText,
    #[ts(type = "number")]
    pub created_at: i64,
}
