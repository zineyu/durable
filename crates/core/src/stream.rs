use tokio::sync::broadcast;

use crate::event::FactEvent;
use crate::id::{MessageId, SessionId};

/// 事件流的统一载荷：已持久化的事实事件与仅实时传输的 token 增量。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum StreamEvent {
    Fact {
        #[ts(type = "number")]
        id: i64,
        session_id: Option<SessionId>,
        #[ts(type = "number")]
        timestamp: i64,
        event: FactEvent,
    },
    Delta {
        session_id: SessionId,
        #[ts(type = "number")]
        message_id: MessageId,
        text: String,
        #[ts(type = "number")]
        timestamp: i64,
    },
    /// 供应商限流后的重试等待。仅实时传输：不落库，不推进游标。
    Retry {
        session_id: SessionId,
        /// 即将进行的重试序号，从 1 开始。
        attempt: u32,
        /// 重试次数上限。
        max_attempts: u32,
        /// 本次等待时长。
        #[ts(type = "number")]
        delay_ms: u64,
    },
    Resync {
        reason: ResyncReason,
    },
}

/// 服务端要求客户端重连并刷新快照的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ResyncReason {
    /// 订阅者落后超出总线容量；`skipped` 是跳过的事件数。
    Lagged {
        #[ts(type = "number")]
        skipped: u64,
    },
}

/// 进程内事件总线。事实事件在提交成功后发布；增量不落库。
#[derive(Clone)]
pub struct EventBus {
    sender: broadcast::Sender<StreamEvent>,
}

impl EventBus {
    /// 创建总线。
    ///
    /// 容量是每个订阅者最多缓冲的事件数；落后于容量的订阅者收到 `Lagged`。
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<StreamEvent> {
        self.sender.subscribe()
    }

    /// 发布事件；没有订阅者时丢弃。
    pub fn publish(&self, event: StreamEvent) {
        let _ = self.sender.send(event);
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(4096)
    }
}
