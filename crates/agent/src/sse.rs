//! agent 的单一全局事件流：回放、实时转发与 resync。
//! 工具结果不经此流，经 link 帧回到 run 引擎。

use std::time::Duration;

use axum::extract::{Query, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use durable::{ResyncReason, StreamEvent};
use futures_util::StreamExt;
use futures_util::stream::{self, Stream};
use tokio::sync::broadcast::error::RecvError;

use crate::api::AgentState;
use crate::error::ApiError;

/// SSE 保活间隔。
const KEEP_ALIVE: Duration = Duration::from_secs(15);

#[derive(Debug, serde::Deserialize)]
pub(crate) struct EventsQuery {
    #[serde(default)]
    pub after: i64,
}

/// 单一全局事件流。先回放 `after` 之后的事实事件，再接入实时事件。
pub(crate) async fn events(
    State(state): State<AgentState>,
    Query(query): Query<EventsQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, serde_json::Error>>>, ApiError> {
    if query.after < 0 {
        return Err(ApiError::invalid_request(
            axum::http::StatusCode::BAD_REQUEST,
            "after 不能为负数".to_owned(),
        ));
    }
    let mut receiver = state.events().subscribe();
    let replayed = state.store().events_after(query.after).await?;
    let max_replayed = replayed
        .iter()
        .map(|event| event.id)
        .max()
        .unwrap_or(query.after);
    let replay = stream::iter(replayed.into_iter().map(move |event| {
        sse_event(&StreamEvent::Fact {
            id: event.id,
            session_id: event.event.session_id().cloned(),
            session: event.session,
            timestamp: event.created_at,
            event: event.event,
        })
    }));
    let live = async_stream::stream! {
        loop {
            match receiver.recv().await {
                Ok(event) => {
                    if forwards(&event, max_replayed) {
                        yield sse_event(&event);
                    }
                }
                Err(RecvError::Lagged(skipped)) => {
                    yield sse_event(&resync_event(skipped));
                    break;
                }
                Err(RecvError::Closed) => break,
            }
        }
    };
    Ok(Sse::new(replay.chain(live))
        .keep_alive(KeepAlive::new().interval(KEEP_ALIVE).text("keep-alive")))
}

/// 广播落后时通知客户端重连并刷新快照。
fn resync_event(skipped: u64) -> StreamEvent {
    StreamEvent::Resync {
        reason: ResyncReason::Lagged { skipped },
    }
}

/// 回放已覆盖的事实事件不再转发；增量与重试提示不落库，无 id，始终转发。
fn forwards(event: &StreamEvent, max_replayed: i64) -> bool {
    match event {
        StreamEvent::Fact { id, .. } => *id > max_replayed,
        StreamEvent::Delta { .. } | StreamEvent::Retry { .. } | StreamEvent::Resync { .. } => true,
    }
}

fn sse_event(event: &StreamEvent) -> Result<Event, serde_json::Error> {
    let data = serde_json::to_string(event)?;
    Ok(match event {
        StreamEvent::Fact { id, .. } => Event::default().id(id.to_string()).data(data),
        StreamEvent::Delta { .. } | StreamEvent::Retry { .. } | StreamEvent::Resync { .. } => {
            Event::default().data(data)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use durable::{FactEvent, SessionId};

    fn deleted(id: i64) -> StreamEvent {
        StreamEvent::Fact {
            id,
            session_id: None,
            session: None,
            timestamp: id,
            event: FactEvent::SessionDeleted {
                session_id: SessionId::parse("s-1").expect("合法 session id"),
            },
        }
    }

    #[test]
    fn forwards_events_after_replay_and_skips_covered_ones() {
        assert!(forwards(&deleted(11), 10));
        assert!(!forwards(&deleted(10), 10));
        assert!(!forwards(&deleted(9), 10));
        assert!(forwards(
            &StreamEvent::Delta {
                session_id: SessionId::parse("s-1").expect("合法 session id"),
                message_id: durable::MessageId::parse(2).expect("合法消息 id"),
                kind: durable::DeltaKind::Text,
                text: "增量".to_owned(),
                timestamp: 9,
            },
            10,
        ));
    }

    #[test]
    fn resync_event_carries_reason() {
        let data = serde_json::to_string(&resync_event(3)).expect("事件可序列化");
        assert_eq!(
            data,
            r#"{"type":"resync","reason":{"kind":"lagged","skipped":3}}"#
        );
    }
}
