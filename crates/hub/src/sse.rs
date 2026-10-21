use std::time::Duration;

use axum::extract::{Query, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use durable::ResyncReason;
use futures_util::StreamExt;
use futures_util::stream::Stream;
use tokio::sync::broadcast::error::RecvError;

use crate::bus::Subscription;
use crate::protocol::HubStreamEvent;
use crate::{ApiError, HubState};

/// SSE 保活间隔。
const KEEP_ALIVE: Duration = Duration::from_secs(15);

#[derive(Debug, serde::Deserialize)]
pub(crate) struct EventsQuery {
    #[serde(default)]
    pub after: u64,
}

/// 单一合并事件流。先回放窗口内缺失的事实事件，再接入实时广播。
///
/// # Errors
///
/// - `ApiError::internal`：hub 的状态锁已被毒化。
pub(crate) async fn events(
    State(state): State<HubState>,
    Query(query): Query<EventsQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, ApiError>>>, ApiError> {
    let subscription = state.bus.subscribe(query.after)?;
    let frames = stream_of(subscription).map(|event| sse_event(&event));
    Ok(Sse::new(frames).keep_alive(KeepAlive::new().interval(KEEP_ALIVE).text("keep-alive")))
}

/// 把一个订阅展开成事件序列：resync 优先，其次回放，最后实时。
fn stream_of(subscription: Subscription) -> impl Stream<Item = HubStreamEvent> {
    async_stream::stream! {
        if let Some(reason) = subscription.resync {
            yield HubStreamEvent::Resync { reason };
            return;
        }
        for event in subscription.replay {
            yield event;
        }
        let Some(mut receiver) = subscription.receiver else {
            return;
        };
        loop {
            match receiver.recv().await {
                Ok(event) => yield event,
                Err(RecvError::Lagged(skipped)) => {
                    yield HubStreamEvent::Resync {
                        reason: ResyncReason::Lagged { skipped },
                    };
                    break;
                }
                Err(RecvError::Closed) => break,
            }
        }
    }
}

fn sse_event(event: &HubStreamEvent) -> Result<Event, ApiError> {
    let data = serde_json::to_string(event)
        .map_err(|error| ApiError::internal(format!("事件无法序列化: {error}")))?;
    let event = match event {
        HubStreamEvent::Fact { hub_seq, .. }
        | HubStreamEvent::NodeRegistered { hub_seq, .. }
        | HubStreamEvent::NodeRemoved { hub_seq, .. }
        | HubStreamEvent::NodeStatusChanged { hub_seq, .. } => {
            Event::default().id(hub_seq.to_string()).data(data)
        }
        HubStreamEvent::Delta { .. }
        | HubStreamEvent::Retry { .. }
        | HubStreamEvent::Resync { .. } => Event::default().data(data),
    };
    Ok(event)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use durable::NodeName;
    use futures_util::StreamExt;

    use super::*;
    use crate::bus::Bus;

    fn name(raw: &str) -> NodeName {
        NodeName::parse(raw).expect("合法节点名")
    }

    /// 规格 user story H-35：落后的订阅者收到 resync 后流结束。
    #[tokio::test]
    async fn lagged_receiver_yields_resync_then_ends() {
        let bus = Arc::new(Bus::new(1, 8));
        let subscription = bus.subscribe(0).expect("订阅广播");
        let mut events = Box::pin(stream_of(subscription));

        bus.node_removed(name("laptop")).expect("广播节点删除");
        bus.node_removed(name("server")).expect("广播节点删除");

        let first = events.next().await.expect("有事件");
        assert!(
            matches!(first, HubStreamEvent::Resync { .. }),
            "落后订阅者必须收到 resync: {first:?}"
        );
        assert!(events.next().await.is_none(), "resync 之后流必须结束");
    }

    /// 规格 user story H-33：窗口外游标只收到一条 resync。
    #[tokio::test]
    async fn resync_subscription_yields_single_resync() {
        let bus = Arc::new(Bus::new(8, 1));
        bus.node_removed(name("laptop")).expect("广播节点删除");
        bus.node_removed(name("server")).expect("广播节点删除");

        let mut events = Box::pin(stream_of(bus.subscribe(0).expect("订阅广播")));
        let first = events.next().await.expect("有事件");
        assert!(matches!(first, HubStreamEvent::Resync { .. }));
        assert!(events.next().await.is_none());
    }

    /// 规格 user story H-31：窗口内游标先收到回放，再接实时。
    #[tokio::test]
    async fn replay_precedes_live_events() {
        let bus = Arc::new(Bus::new(8, 8));
        bus.node_removed(name("laptop")).expect("广播节点删除");

        let mut events = Box::pin(stream_of(bus.subscribe(0).expect("订阅广播")));
        let replayed = events.next().await.expect("有回放事件");
        assert_eq!(hub_seq(&replayed), 1, "回放应带第一个序号");

        bus.node_removed(name("server")).expect("广播节点删除");
        let live = events.next().await.expect("有实时事件");
        assert_eq!(hub_seq(&live), 2, "实时事件序号必须递增");
    }

    fn hub_seq(event: &HubStreamEvent) -> u64 {
        match event {
            HubStreamEvent::Fact { hub_seq, .. }
            | HubStreamEvent::NodeRegistered { hub_seq, .. }
            | HubStreamEvent::NodeRemoved { hub_seq, .. }
            | HubStreamEvent::NodeStatusChanged { hub_seq, .. } => *hub_seq,
            HubStreamEvent::Delta { .. }
            | HubStreamEvent::Retry { .. }
            | HubStreamEvent::Resync { .. } => 0,
        }
    }
}
