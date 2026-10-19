use std::convert::Infallible;
use std::time::Duration;

use axum::extract::{Query, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use durable::StreamEvent;
use futures_util::StreamExt;
use futures_util::stream::{self, Stream};
use tokio::sync::broadcast::error::RecvError;

use crate::api::AgentState;
use crate::error::ApiError;

/// SSE 保活间隔。
const KEEP_ALIVE: Duration = Duration::from_secs(15);

#[derive(Debug, serde::Deserialize)]
pub struct EventsQuery {
    #[serde(default)]
    pub after: i64,
}

/// 单一全局事件流。先回放 `after` 之后的事实事件，再接入实时事件。
pub async fn events(
    State(state): State<AgentState>,
    Query(query): Query<EventsQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
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
        Ok(sse_event(&StreamEvent::Fact {
            id: event.id,
            session_id: event.event.session_id().cloned(),
            timestamp: event.created_at,
            event: event.event,
        }))
    }));
    let live = async_stream::stream! {
        loop {
            match receiver.recv().await {
                Ok(event) => {
                    if let StreamEvent::Fact { id, .. } = &event
                        && *id <= max_replayed
                    {
                        continue;
                    }
                    yield Ok(sse_event(&event));
                }
                Err(RecvError::Lagged(count)) => {
                    yield Ok(Event::default().comment(format!("lagged {count}")));
                }
                Err(RecvError::Closed) => break,
            }
        }
    };
    Ok(Sse::new(replay.chain(live))
        .keep_alive(KeepAlive::new().interval(KEEP_ALIVE).text("keep-alive")))
}

fn sse_event(event: &StreamEvent) -> Event {
    let data = serde_json::to_string(event).expect("事件可序列化");
    match event {
        StreamEvent::Fact { id, .. } => Event::default().id(id.to_string()).data(data),
        StreamEvent::Delta { .. } => Event::default().data(data),
    }
}
