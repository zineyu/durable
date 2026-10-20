use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use agent::HealthResponse;
use durable::{NodeMetadata, NodeName, ResyncReason, SessionId, StreamEvent};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

use crate::protocol::{HubStreamEvent, NodeStatus, REQUIRED_AGENT_PROTOCOL_VERSION};

/// 一个客户端订阅时得到的内容：可回放的事件、实时接收端，或一条 resync 指示。
pub struct Subscription {
    pub replay: Vec<HubStreamEvent>,
    pub receiver: Option<broadcast::Receiver<HubStreamEvent>>,
    pub resync: Option<ResyncReason>,
}

#[derive(Debug, Clone)]
pub struct NodeView {
    pub status: NodeStatus,
    pub detail: Option<String>,
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
    views: HashMap<NodeName, NodeView>,
}

impl Bus {
    pub fn new(broadcast_capacity: usize, replay_window: usize) -> Self {
        let (sender, _) = broadcast::channel(broadcast_capacity);
        Self {
            sender,
            state: Mutex::new(BusState {
                seq: 0,
                window: VecDeque::new(),
                views: HashMap::new(),
            }),
            replay_window,
        }
    }

    /// 当前最大广播序号。客户端用它作为续传游标的起点。
    pub fn max_seq(&self) -> u64 {
        self.state.lock().expect("bus 锁").seq
    }

    pub fn view(&self, name: &NodeName) -> NodeView {
        self.state
            .lock()
            .expect("bus 锁")
            .views
            .get(name)
            .cloned()
            .unwrap_or(NodeView {
                status: NodeStatus::Unreachable,
                detail: Some("尚未连接".to_owned()),
            })
    }

    /// 订阅广播。`after` 落在回放窗口之外时返回 resync，而不是回放。
    pub fn subscribe(&self, after: u64) -> Subscription {
        let state = self.state.lock().expect("bus 锁");
        let seq = state.seq;
        let window_start = match state.window.front() {
            Some(event) => seq_of(event),
            None => seq.saturating_add(1),
        };
        let outside = after > seq || (after + 1 < window_start);
        let receiver = self.sender.subscribe();
        if outside {
            return Subscription {
                replay: Vec::new(),
                receiver: None,
                resync: Some(ResyncReason::Lagged {
                    skipped: seq.saturating_sub(after),
                }),
            };
        }
        let replay = state
            .window
            .iter()
            .filter(|event| seq_of(event) > after)
            .cloned()
            .collect();
        Subscription {
            replay,
            receiver: Some(receiver),
            resync: None,
        }
    }

    pub fn set_view(&self, name: NodeName, view: NodeView) {
        let previous = self
            .state
            .lock()
            .expect("bus 锁")
            .views
            .insert(name.clone(), view.clone());
        if previous.as_ref().map(|old| old.status) == Some(view.status) {
            return;
        }
        match view.status {
            NodeStatus::Ok => tracing::info!(node = %name, "节点可用"),
            status => tracing::warn!(
                node = %name,
                status = ?status,
                detail = view.detail.as_deref().unwrap_or(""),
                "节点不可用"
            ),
        }
        self.publish_fact(|hub_seq| HubStreamEvent::NodeStatusChanged {
            hub_seq,
            name,
            status: view.status,
            detail: view.detail,
        });
    }

    pub fn node_registered(&self, node: NodeMetadata) {
        self.publish_fact(|hub_seq| HubStreamEvent::NodeRegistered { hub_seq, node });
    }

    pub fn node_removed(&self, name: NodeName) {
        self.state.lock().expect("bus 锁").views.remove(&name);
        self.publish_fact(|hub_seq| HubStreamEvent::NodeRemoved { hub_seq, name });
    }

    pub fn node_fact(
        &self,
        node: NodeName,
        id: i64,
        session_id: Option<SessionId>,
        timestamp: i64,
        event: durable::FactEvent,
    ) {
        self.publish_fact(|hub_seq| HubStreamEvent::Fact {
            hub_seq,
            node,
            id,
            session_id,
            timestamp,
            event,
        });
    }

    pub fn delta(
        &self,
        node: NodeName,
        session_id: SessionId,
        message_id: durable::MessageId,
        text: String,
        timestamp: i64,
    ) {
        let _ = self.sender.send(HubStreamEvent::Delta {
            node,
            session_id,
            message_id,
            text,
            timestamp,
        });
    }

    fn publish_fact(&self, build: impl FnOnce(u64) -> HubStreamEvent) {
        let mut state = self.state.lock().expect("bus 锁");
        state.seq += 1;
        let event = build(state.seq);
        state.window.push_back(event.clone());
        while state.window.len() > self.replay_window {
            state.window.pop_front();
        }
        let _ = self.sender.send(event);
    }
}

fn seq_of(event: &HubStreamEvent) -> u64 {
    match event {
        HubStreamEvent::Fact { hub_seq, .. }
        | HubStreamEvent::NodeRegistered { hub_seq, .. }
        | HubStreamEvent::NodeRemoved { hub_seq, .. }
        | HubStreamEvent::NodeStatusChanged { hub_seq, .. } => *hub_seq,
        HubStreamEvent::Delta { .. } | HubStreamEvent::Resync { .. } => 0,
    }
}

/// 为单个节点维持常驻上游事件流，带指数退避重连。
pub struct NodeWatcher {
    handle: JoinHandle<()>,
}

impl NodeWatcher {
    pub fn start(bus: Arc<Bus>, node: NodeMetadata) -> Self {
        let handle = tokio::spawn({
            let client = reqwest::Client::new();
            async move { watch_forever(bus, node, client).await }
        });
        Self { handle }
    }
}

impl Drop for NodeWatcher {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn watch_forever(bus: Arc<Bus>, node: NodeMetadata, client: reqwest::Client) {
    let base = node.url.trim_end_matches('/').to_owned();
    let mut last_id = 0i64;
    let mut attempt = 0u32;
    loop {
        let was_ok = bus.view(&node.name).status == NodeStatus::Ok;
        if let Err(failure) = run_stream(&bus, &client, &base, &node.name, &mut last_id).await {
            bus.set_view(
                node.name.clone(),
                NodeView {
                    status: failure.status,
                    detail: Some(failure.detail),
                },
            );
        }
        attempt = if was_ok {
            1
        } else {
            attempt.saturating_add(1).min(5)
        };
        tokio::time::sleep(Duration::from_secs(1u64 << attempt.min(5))).await;
    }
}

/// 探测或订阅失败。状态由失败本身决定，避免上层把它抹平成不可达。
struct Failure {
    status: NodeStatus,
    detail: String,
}

impl Failure {
    fn unreachable(detail: impl Into<String>) -> Self {
        Self {
            status: NodeStatus::Unreachable,
            detail: detail.into(),
        }
    }

    fn incompatible(detail: impl Into<String>) -> Self {
        Self {
            status: NodeStatus::Incompatible,
            detail: detail.into(),
        }
    }
}

async fn run_stream(
    bus: &Arc<Bus>,
    client: &reqwest::Client,
    base: &str,
    name: &NodeName,
    last_id: &mut i64,
) -> Result<(), Failure> {
    let health = client
        .get(format!("{base}/api/health"))
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .map_err(|error| Failure::unreachable(format!("连接节点失败: {error}")))?;
    let health: HealthResponse = health
        .json()
        .await
        .map_err(|error| Failure::unreachable(format!("health 响应无法解析: {error}")))?;
    if health.protocol_version != REQUIRED_AGENT_PROTOCOL_VERSION {
        return Err(Failure::incompatible(format!(
            "agent 协议版本 {} 与 hub 要求的 {} 不一致",
            health.protocol_version, REQUIRED_AGENT_PROTOCOL_VERSION
        )));
    }
    bus.set_view(
        name.clone(),
        NodeView {
            status: NodeStatus::Ok,
            detail: None,
        },
    );

    let response = client
        .get(format!("{base}/api/events?after={last_id}"))
        .send()
        .await
        .map_err(|error| Failure::unreachable(format!("订阅事件流失败: {error}")))?;
    if !response.status().is_success() {
        return Err(Failure::unreachable(format!(
            "事件流返回 {}",
            response.status()
        )));
    }
    let mut stream = response.bytes_stream();
    let mut buffer: Vec<u8> = Vec::new();
    loop {
        use futures_util::StreamExt;
        let chunk = match stream.next().await {
            Some(Ok(chunk)) => chunk,
            Some(Err(error)) => {
                return Err(Failure::unreachable(format!("读取事件流失败: {error}")));
            }
            None => return Err(Failure::unreachable("事件流结束")),
        };
        buffer.extend_from_slice(&chunk);
        while let Some(end) = buffer.windows(2).position(|window| window == b"\n\n") {
            let frame: Vec<u8> = buffer.drain(..end + 2).collect();
            let text = String::from_utf8_lossy(&frame);
            for line in text.lines() {
                let Some(payload) = line.strip_prefix("data: ") else {
                    continue;
                };
                let Ok(event) = serde_json::from_str::<StreamEvent>(payload) else {
                    continue;
                };
                match event {
                    StreamEvent::Fact {
                        id,
                        session_id,
                        timestamp,
                        event,
                    } => {
                        *last_id = id;
                        bus.node_fact(name.clone(), id, session_id, timestamp, event);
                    }
                    StreamEvent::Delta {
                        session_id,
                        message_id,
                        text,
                        timestamp,
                    } => bus.delta(name.clone(), session_id, message_id, text, timestamp),
                    StreamEvent::Resync { .. } => {
                        return Err(Failure::unreachable("上游要求 resync"));
                    }
                }
            }
        }
    }
}
