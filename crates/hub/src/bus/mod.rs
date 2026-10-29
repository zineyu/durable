//! hub 的广播总线：分发、序号、回放窗口、共享状态字段与节点条目的门面。
//! 上游订阅与重连在 `watch`，游标与回放窗口在 `window`，节点条目与写入规则在 `node`。

mod node;
mod watch;
mod window;

pub(crate) use self::node::DISABLED_DETAIL;
pub use self::node::NodeView;
use self::node::{EpochClock, NodeEntry};
pub(crate) use self::watch::NodeWatcher;

use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, MutexGuard};

use durable::{ResyncReason, SessionId};

use crate::node::NodeName;
use tokio::sync::broadcast;

use crate::ApiError;
use crate::protocol::HubStreamEvent;

/// 一个客户端订阅时得到的内容：可回放的事件、实时接收端，或一条 resync 指示。
pub struct Subscription {
    pub replay: Vec<HubStreamEvent>,
    pub receiver: Option<broadcast::Receiver<HubStreamEvent>>,
    pub resync: Option<ResyncReason>,
}

/// hub 的广播总线：分配广播序号、保留有界回放窗口、向所有客户端分发。
pub struct Bus {
    sender: broadcast::Sender<HubStreamEvent>,
    state: Mutex<BusState>,
    replay_window: usize,
}

struct BusState {
    seq: u64,
    window: VecDeque<HubStreamEvent>,
    /// 每个节点一条记录。视图、计数与纪元同生同灭。
    nodes: HashMap<NodeName, NodeEntry>,
    /// 视图纪元的唯一来源。跨条目与跨删除都不重计。
    clock: EpochClock,
}

impl Bus {
    #[must_use]
    pub fn new(broadcast_capacity: usize, replay_window: usize) -> Self {
        let (sender, _) = broadcast::channel(broadcast_capacity);
        Self {
            sender,
            state: Mutex::new(BusState {
                seq: 0,
                window: VecDeque::new(),
                nodes: HashMap::new(),
                clock: EpochClock::default(),
            }),
            replay_window,
        }
    }

    /// 取总线状态锁。
    ///
    /// 持锁线程 panic 会毒化锁。毒化之后共享状态不再可信，所以作为内部错误上报，而不是恢复使用。
    ///
    /// # Errors
    ///
    /// - `ApiError::internal`：hub 的状态锁已被毒化。
    fn state(&self) -> Result<MutexGuard<'_, BusState>, ApiError> {
        self.state
            .lock()
            .map_err(|_| ApiError::internal("bus 状态锁已毒化".to_owned()))
    }

    /// 广播一条事实事件。
    ///
    /// # Errors
    ///
    /// - `ApiError::internal`：hub 的状态锁已被毒化。
    pub fn node_fact(
        &self,
        node: NodeName,
        id: i64,
        session_id: Option<SessionId>,
        session: Option<durable::SessionMetadata>,
        timestamp: i64,
        event: durable::FactEvent,
    ) -> Result<(), ApiError> {
        self.record_fact_time(&node, timestamp)?;
        self.publish_fact(|hub_seq| HubStreamEvent::Fact {
            hub_seq,
            node,
            id,
            session_id,
            session,
            timestamp,
            event: Box::new(event),
        })
    }

    /// 转发增量。仅实时，不进回放窗口。
    pub fn delta(
        &self,
        node: NodeName,
        session_id: SessionId,
        message_id: durable::MessageId,
        kind: durable::DeltaKind,
        text: String,
        timestamp: i64,
    ) {
        self.broadcast(HubStreamEvent::Delta {
            node,
            session_id,
            message_id,
            kind,
            text,
            timestamp,
        });
    }

    /// 转发重试提示。仅实时，不进回放窗口。
    pub fn retry(
        &self,
        node: NodeName,
        session_id: SessionId,
        attempt: u32,
        max_attempts: u32,
        delay_ms: u64,
    ) {
        self.broadcast(HubStreamEvent::Retry {
            node,
            session_id,
            attempt,
            max_attempts,
            delay_ms,
        });
    }

    /// 分配广播序号，进入回放窗口后广播。
    ///
    /// # Errors
    ///
    /// - `ApiError::internal`：hub 的状态锁已被毒化。
    fn publish_fact(&self, build: impl FnOnce(u64) -> HubStreamEvent) -> Result<(), ApiError> {
        let mut state = self.state()?;
        state.seq += 1;
        let event = build(state.seq);
        state.window.push_back(event.clone());
        while state.window.len() > self.replay_window {
            state.window.pop_front();
        }
        self.broadcast(event);
        Ok(())
    }

    /// 尽力广播。没有订阅者时 `send` 返回错误，这不是失败路径，因此只记录接收者数量。
    fn broadcast(&self, event: HubStreamEvent) {
        let _receivers = self.sender.send(event);
    }
}
