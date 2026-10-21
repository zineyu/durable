//! 上游假件：可编程的假 node 与合并事件流客户端。

#![expect(
    dead_code,
    clippy::expect_used,
    clippy::panic,
    clippy::let_underscore_must_use,
    reason = "测试脚手架用 expect 与 panic 直接报告夹具与断言失败：夹具失败必须立刻暴露。\
              本模块被每个集成测试目标独立编译，条目未必都被使用。"
)]

use std::net::Ipv4Addr;
use std::time::Duration;

use axum::routing::get;
use axum::{Json, Router};

use super::harness::Harness;

/// 只实现 health 与 bootstrap 的假 agent，用于构造协议版本不匹配的场景。
pub(crate) async fn start_fake_agent(protocol_version: u32) -> String {
    let router = Router::new()
        .route(
            "/api/health",
            get(move || async move {
                Json(serde_json::json!({
                    "status": "ok",
                    "version": "0.0.0",
                    "protocolVersion": protocol_version,
                }))
            }),
        )
        .route(
            "/api/bootstrap",
            get(|| async {
                Json(serde_json::json!({
                    "model": "fake",
                    "maxEventId": 0,
                    "workspaces": [],
                    "sessions": [],
                }))
            }),
        );
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("绑定假 agent 端口");
    let address = listener.local_addr().expect("读取假 agent 地址");
    tokio::spawn(async move {
        axum::serve(listener, router).await.expect("假 agent 运行");
    });
    format!("http://{address}")
}

/// hub 合并事件流的测试客户端。
pub(crate) struct EventStream {
    stream:
        std::pin::Pin<Box<dyn futures_util::Stream<Item = reqwest::Result<bytes::Bytes>> + Send>>,
    buffer: Vec<u8>,
}

impl EventStream {
    pub(crate) async fn connect(harness: &Harness, after: u64) -> Self {
        let response = reqwest::get(harness.url(&format!("/api/events?after={after}")))
            .await
            .expect("订阅合并事件流");
        assert_eq!(
            response.status(),
            reqwest::StatusCode::OK,
            "订阅合并事件流应返回 200"
        );
        Self {
            stream: Box::pin(response.bytes_stream()),
            buffer: Vec::new(),
        }
    }

    /// 读取下一条事件。
    pub(crate) async fn next_event(&mut self) -> hub::HubStreamEvent {
        use futures_util::StreamExt;

        let read = async {
            loop {
                if let Some(end) = self.buffer.windows(2).position(|window| window == b"\n\n") {
                    let frame: Vec<u8> = self.buffer.drain(..end + 2).collect();
                    let text = String::from_utf8_lossy(&frame).to_string();
                    for line in text.lines() {
                        if let Some(payload) = line.strip_prefix("data: ") {
                            return serde_json::from_str::<hub::HubStreamEvent>(payload)
                                .expect("解析合并事件");
                        }
                    }
                    continue;
                }
                let chunk = self
                    .stream
                    .next()
                    .await
                    .expect("事件流结束")
                    .expect("读取事件流");
                self.buffer.extend_from_slice(&chunk);
            }
        };
        tokio::time::timeout(Duration::from_secs(10), read)
            .await
            .expect("等待事件超时")
    }

    /// 读取事件直到满足条件。
    pub(crate) async fn next_matching(
        &mut self,
        mut predicate: impl FnMut(&hub::HubStreamEvent) -> bool,
    ) -> hub::HubStreamEvent {
        for _ in 0..50 {
            let event = self.next_event().await;
            if predicate(&event) {
                return event;
            }
        }
        panic!("未等到满足条件的事件");
    }
}

/// 可编程的假 node：把上游时序变成事件驱动，用于覆盖游标、状态说明与超时。
///
/// - `/api/health` 永远报协议版本一致。
/// - `/api/bootstrap` 可由闸门挂起，`maxEventId` 是测试设定的水位。
/// - `/api/events` 记录收到的 `after`，先回放该游标之后的事实，再接实时推送；
///   广播落后时像 `agent` 一样发一条 `resync` 并结束该流。
/// - 其余路径永不响应，用于覆盖命令转发超时。
pub(crate) mod scripted {
    use std::convert::Infallible;
    use std::net::{Ipv4Addr, SocketAddr};
    use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
    use std::sync::{Arc, Mutex};

    use axum::extract::Query;
    use axum::http::StatusCode;
    use axum::response::sse::{Event, Sse};
    use axum::response::{IntoResponse, Response};
    use axum::routing::get;
    use axum::{Json, Router};
    use futures_util::StreamExt;
    use tokio::sync::{Notify, broadcast, oneshot};

    /// 事件流的行为。
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum EventsMode {
        /// 按脚本推送事实。
        Scripted,
        /// 连接后立即结束。
        End,
        /// 接受连接但永不返回响应头。
        Stall,
    }

    /// hub 的订阅请求。记录游标是否为缺失，因此能区分「没带游标」与「带 0」。
    #[derive(Debug, serde::Deserialize)]
    pub(crate) struct EventsQuery {
        pub after: Option<i64>,
    }

    struct Inner {
        watermark: AtomicI64,
        mode: Mutex<EventsMode>,
        after: Mutex<Vec<Option<i64>>>,
        facts: broadcast::Sender<durable::StreamEvent>,
        published: Mutex<Vec<durable::StreamEvent>>,
        health_body: Mutex<Option<String>>,
        bootstrap_stall: AtomicBool,
        bootstrap_wait: Mutex<Option<oneshot::Receiver<()>>>,
        bootstrap_release: Mutex<Option<oneshot::Sender<()>>>,
        bootstrap_started: Notify,
    }

    pub(crate) struct ScriptedNode {
        pub url: String,
        address: SocketAddr,
        inner: Arc<Inner>,
        task: tokio::task::AbortHandle,
    }

    impl ScriptedNode {
        pub(crate) async fn start() -> Self {
            Self::start_with_fact_capacity(64).await
        }

        /// 用指定的广播容量启动。
        ///
        /// 容量小于一次推送的事实数时，hub 的订阅会收到 `Lagged`，从而模拟
        /// 「hub 落后于 agent 的内存广播」这一真实场景。
        pub(crate) async fn start_with_fact_capacity(capacity: usize) -> Self {
            let inner = Arc::new(Inner {
                watermark: AtomicI64::new(0),
                mode: Mutex::new(EventsMode::Scripted),
                after: Mutex::new(Vec::new()),
                facts: broadcast::channel(capacity).0,
                published: Mutex::new(Vec::new()),
                health_body: Mutex::new(None),
                bootstrap_stall: AtomicBool::new(false),
                bootstrap_wait: Mutex::new(None),
                bootstrap_release: Mutex::new(None),
                bootstrap_started: Notify::new(),
            });

            let router =
                Router::new()
                    .route(
                        "/api/health",
                        get({
                            let inner = Arc::clone(&inner);
                            move || async move {
                                let body = inner.health_body.lock().expect("健康说明锁").clone();
                                match body {
                                    Some(body) => (
                                        StatusCode::OK,
                                        [(axum::http::header::CONTENT_TYPE, "application/json")],
                                        body,
                                    )
                                        .into_response(),
                                    None => Json(serde_json::json!({
                                        "status": "ok",
                                        "version": "0.0.0",
                                        "protocolVersion": hub::REQUIRED_AGENT_PROTOCOL_VERSION,
                                    }))
                                    .into_response(),
                                }
                            }
                        }),
                    )
                    .route("/api/stall", get(|| async { stalled_response() }))
                    .route(
                        "/api/bootstrap",
                        get({
                            let inner = Arc::clone(&inner);
                            move || async move {
                                if inner.bootstrap_stall.load(Ordering::SeqCst) {
                                    return stalled_response();
                                }
                                let waiting = inner.bootstrap_wait.lock().expect("闸门锁").take();
                                if let Some(receiver) = waiting {
                                    inner.bootstrap_started.notify_one();
                                    let _ = receiver.await;
                                }
                                Json(serde_json::json!({
                                    "model": "scripted",
                                    "maxEventId": inner.watermark.load(Ordering::SeqCst),
                                    "workspaces": [],
                                    "sessions": [],
                                }))
                                .into_response()
                            }
                        }),
                    )
                    .route(
                        "/api/events",
                        get({
                            let inner = Arc::clone(&inner);
                            move |Query(query): Query<EventsQuery>| async move {
                                events(inner, query).await
                            }
                        }),
                    )
                    .fallback(|| async { std::future::pending::<()>().await });

            let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
                .await
                .expect("绑定假节点端口");
            let address = listener.local_addr().expect("读取假节点地址");
            let task = tokio::spawn(async move {
                axum::serve(listener, router).await.expect("假节点运行");
            });
            Self {
                url: format!("http://{address}"),
                address,
                inner,
                task: task.abort_handle(),
            }
        }

        /// 设定 bootstrap 返回的 `maxEventId`，即上游水位。
        pub(crate) fn set_watermark(&self, value: i64) {
            self.inner.watermark.store(value, Ordering::SeqCst);
        }

        /// 停止监听。之后对该端口的连接被拒绝，用于构造「连接失败」这一类原因。
        pub(crate) fn stop(&self) {
            self.task.abort();
        }

        /// 让 bootstrap 只返回响应头、body 永不结束，用于覆盖探测的读体时限。
        pub(crate) fn stall_bootstrap_body(&self) {
            self.inner.bootstrap_stall.store(true, Ordering::SeqCst);
        }

        /// 等待端口真正拒绝连接。stop 的 abort 是异步生效的。
        pub(crate) async fn wait_until_refused(&self) {
            for _ in 0..200 {
                if tokio::net::TcpStream::connect(self.address).await.is_err() {
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
            panic!("假节点未在超时内停止监听");
        }

        /// 让健康检查返回指定原文。原文按 JSON 反序列化，非法内容触发「health 响应无法解析」。
        pub(crate) fn set_health_body(&self, body: &str) {
            *self.inner.health_body.lock().expect("健康说明锁") = Some(body.to_owned());
        }

        pub(crate) fn set_events_mode(&self, mode: EventsMode) {
            *self.inner.mode.lock().expect("模式锁") = mode;
        }

        /// 上游收到的全部游标。缺失游标的请求记为 None。
        pub(crate) fn after_requests(&self) -> Vec<Option<i64>> {
            self.inner.after.lock().expect("after 锁").clone()
        }

        /// 推送一条重试提示。仅实时，像 `agent` 一样不进入回放历史。
        pub(crate) fn publish_retry(
            &self,
            session_id: &str,
            attempt: u32,
            max_attempts: u32,
            delay_ms: u64,
        ) {
            self.inner
                .facts
                .send(durable::StreamEvent::Retry {
                    session_id: durable::SessionId::parse(session_id).expect("合法 session id"),
                    attempt,
                    max_attempts,
                    delay_ms,
                })
                .expect("推送重试提示需要上游事件流已订阅");
        }

        /// 向上游推送一条事实。
        pub(crate) fn publish_fact(&self, id: i64, event: durable::FactEvent) {
            self.publish(durable::StreamEvent::Fact {
                id,
                session_id: None,
                timestamp: id,
                event,
            });
        }

        /// 推送一个事件，并记入回放历史。
        fn publish(&self, event: durable::StreamEvent) {
            self.inner
                .published
                .lock()
                .expect("回放历史锁")
                .push(event.clone());
            self.inner
                .facts
                .send(event)
                .expect("推送事件需要上游事件流已订阅");
        }

        /// 挂起下一次 bootstrap 响应，直到 `release_bootstrap`。
        pub(crate) fn hold_bootstrap(&self) {
            let (sender, receiver) = oneshot::channel();
            {
                let mut waiting = self.inner.bootstrap_wait.lock().expect("闸门锁");
                assert!(waiting.is_none(), "已有挂起的 bootstrap 闸门");
                *waiting = Some(receiver);
            }
            *self.inner.bootstrap_release.lock().expect("闸门锁") = Some(sender);
        }

        /// 等待被挂起的 bootstrap 请求到达。
        pub(crate) async fn wait_for_bootstrap_request(&self) {
            self.inner.bootstrap_started.notified().await;
        }

        pub(crate) fn release_bootstrap(&self) {
            let sender = self
                .inner
                .bootstrap_release
                .lock()
                .expect("闸门锁")
                .take()
                .expect("没有待释放的 bootstrap 闸门");
            sender.send(()).expect("闸门接收端已结束");
        }
    }

    /// 一条事实事件，用于验证上游事实进入合并流。
    pub(crate) fn fact() -> durable::FactEvent {
        durable::FactEvent::SessionDeleted {
            session_id: durable::SessionId::parse("s1").expect("合法 session id"),
        }
    }

    /// 等待上游以指定游标订阅。
    pub(crate) async fn wait_for_after(node: &ScriptedNode, expected: i64) {
        for _ in 0..500 {
            if node.after_requests().last().copied().flatten() == Some(expected) {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!(
            "上游未在超时内以 {expected} 订阅，实际 {:?}",
            node.after_requests()
        );
    }

    /// 等待上游事件流建立。请求到达即代表订阅已注册。
    pub(crate) async fn wait_for_upstream(node: &ScriptedNode) {
        for _ in 0..500 {
            if !node.after_requests().is_empty() {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!("上游事件流未在超时内建立");
    }

    /// 只发响应头、body 永不结束的响应。
    fn stalled_response() -> Response {
        let body = axum::body::Body::from_stream(async_stream::stream! {
            std::future::pending::<()>().await;
            yield Ok::<axum::body::Bytes, Infallible>(axum::body::Bytes::new());
        });
        Response::builder()
            .status(StatusCode::OK)
            .header(axum::http::header::CONTENT_TYPE, "application/json")
            .body(body)
            .expect("构造卡住的响应")
    }

    /// 事件携带的事实 id。增量与 resync 没有 id。
    fn event_id(event: &durable::StreamEvent) -> Option<i64> {
        match event {
            durable::StreamEvent::Fact { id, .. } => Some(*id),
            durable::StreamEvent::Delta { .. }
            | durable::StreamEvent::Retry { .. }
            | durable::StreamEvent::Resync { .. } => None,
        }
    }

    fn frame(event: &durable::StreamEvent) -> Event {
        Event::default().data(serde_json::to_string(event).expect("序列化事件"))
    }

    async fn events(inner: Arc<Inner>, query: EventsQuery) -> Response {
        if *inner.mode.lock().expect("模式锁") == EventsMode::Stall {
            std::future::pending::<()>().await;
        }
        let mut receiver = inner.facts.subscribe();
        inner.after.lock().expect("after 锁").push(query.after);
        let after = query.after.unwrap_or(0);
        let replayed: Vec<durable::StreamEvent> = inner
            .published
            .lock()
            .expect("回放历史锁")
            .iter()
            .filter(|event| event_id(event).is_some_and(|id| id > after))
            .cloned()
            .collect();
        let replayed_max = replayed.iter().filter_map(event_id).max();
        match *inner.mode.lock().expect("模式锁") {
            EventsMode::End => {
                Sse::new(futures_util::stream::empty::<Result<Event, Infallible>>()).into_response()
            }
            EventsMode::Stall => (StatusCode::SERVICE_UNAVAILABLE, "已停止响应").into_response(),
            EventsMode::Scripted => {
                let replay = futures_util::stream::iter(
                    replayed
                        .into_iter()
                        .map(|event| Ok::<Event, Infallible>(frame(&event))),
                );
                let live = async_stream::stream! {
                    loop {
                        match receiver.recv().await {
                            Ok(event) => {
                                let covered = replayed_max
                                    .zip(event_id(&event))
                                    .is_some_and(|(max, id)| id <= max);
                                if covered {
                                    continue;
                                }
                                yield Ok::<Event, Infallible>(frame(&event));
                            }
                            Err(broadcast::error::RecvError::Lagged(skipped)) => {
                                yield Ok::<Event, Infallible>(frame(&durable::StreamEvent::Resync {
                                    reason: durable::ResyncReason::Lagged { skipped },
                                }));
                                return;
                            }
                            Err(broadcast::error::RecvError::Closed) => return,
                        }
                    }
                };
                Sse::new(replay.chain(live)).into_response()
            }
        }
    }
}
