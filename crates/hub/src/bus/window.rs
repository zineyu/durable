//! 广播游标与有界回放窗口：`after` 落在窗口内回放，落在窗口外要求 resync。
//! 分配序号在父模块，上游订阅在 `watch`。

use super::{Bus, Subscription};
use crate::ApiError;
use crate::protocol::HubStreamEvent;
use durable::ResyncReason;

impl Bus {
    /// 当前最大广播序号。客户端用它作为续传游标的起点。
    ///
    /// # Errors
    ///
    /// - `ApiError::internal`：hub 的状态锁已被毒化。
    pub fn max_seq(&self) -> Result<u64, ApiError> {
        Ok(self.state()?.seq)
    }

    /// 订阅广播。`after` 落在回放窗口之外时返回 resync，而不是回放。
    ///
    /// # Errors
    ///
    /// - `ApiError::internal`：hub 的状态锁已被毒化。
    pub fn subscribe(&self, after: u64) -> Result<Subscription, ApiError> {
        let state = self.state()?;
        let seq = state.seq;
        let window_start = match state.window.front() {
            Some(event) => seq_of(event),
            None => seq.saturating_add(1),
        };
        let outside = after > seq || (after + 1 < window_start);
        let receiver = self.sender.subscribe();
        if outside {
            return Ok(Subscription {
                replay: Vec::new(),
                receiver: None,
                resync: Some(ResyncReason::Lagged {
                    skipped: seq.saturating_sub(after),
                }),
            });
        }
        let replay = state
            .window
            .iter()
            .filter(|event| seq_of(event) > after)
            .cloned()
            .collect();
        Ok(Subscription {
            replay,
            receiver: Some(receiver),
            resync: None,
        })
    }
}

fn seq_of(event: &HubStreamEvent) -> u64 {
    match event {
        HubStreamEvent::Fact { hub_seq, .. }
        | HubStreamEvent::NodeRegistered { hub_seq, .. }
        | HubStreamEvent::NodeRemoved { hub_seq, .. }
        | HubStreamEvent::NodeUpdated { hub_seq, .. }
        | HubStreamEvent::NodeStatusChanged { hub_seq, .. } => *hub_seq,
        HubStreamEvent::Delta { .. }
        | HubStreamEvent::Retry { .. }
        | HubStreamEvent::Resync { .. } => 0,
    }
}
