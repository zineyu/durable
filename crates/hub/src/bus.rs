use std::collections::{HashMap, VecDeque};
use std::convert::Infallible;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use agent::HealthResponse;
use axum::body::Bytes;
use durable::{NodeMetadata, NodeName, ResyncReason, SessionId, StreamEvent};
use futures_util::StreamExt;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

use crate::node_probe;
use crate::protocol::{HubStreamEvent, NodeStatus, REQUIRED_AGENT_PROTOCOL_VERSION};
use crate::{ApiError, HubOptions};

/// 一个客户端订阅时得到的内容：可回放的事件、实时接收端，或一条 resync 指示。
pub struct Subscription {
    pub replay: Vec<HubStreamEvent>,
    pub receiver: Option<broadcast::Receiver<HubStreamEvent>>,
    pub resync: Option<ResyncReason>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
    #[must_use]
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

    /// 当前最大广播序号。客户端用它作为续传游标的起点。
    ///
    /// # Errors
    ///
    /// - `ApiError::internal`：hub 的状态锁已被毒化。
    pub fn max_seq(&self) -> Result<u64, ApiError> {
        Ok(self.state()?.seq)
    }

    /// 节点视图。没有记录的节点按「尚未连接」的不可达处理。
    ///
    /// # Errors
    ///
    /// - `ApiError::internal`：hub 的状态锁已被毒化。
    pub fn view(&self, name: &NodeName) -> Result<NodeView, ApiError> {
        let state = self.state()?;
        Ok(state.views.get(name).cloned().unwrap_or(NodeView {
            status: NodeStatus::Unreachable,
            detail: Some("尚未连接".to_owned()),
        }))
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

    /// 写入节点视图。只有视图真的变了才广播。
    ///
    /// # Errors
    ///
    /// - `ApiError::internal`：hub 的状态锁已被毒化。
    pub fn set_view(&self, name: NodeName, view: NodeView) -> Result<(), ApiError> {
        let previous = self.state()?.views.insert(name.clone(), view.clone());
        if previous.as_ref() == Some(&view) {
            return Ok(());
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
        })
    }

    /// 记录节点登记并广播。
    ///
    /// # Errors
    ///
    /// - `ApiError::internal`：hub 的状态锁已被毒化。
    pub fn node_registered(&self, node: NodeMetadata) -> Result<(), ApiError> {
        self.publish_fact(|hub_seq| HubStreamEvent::NodeRegistered { hub_seq, node })
    }

    /// 清除节点视图并广播删除。
    ///
    /// # Errors
    ///
    /// - `ApiError::internal`：hub 的状态锁已被毒化。
    pub fn node_removed(&self, name: NodeName) -> Result<(), ApiError> {
        self.state()?.views.remove(&name);
        self.publish_fact(|hub_seq| HubStreamEvent::NodeRemoved { hub_seq, name })
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
        timestamp: i64,
        event: durable::FactEvent,
    ) -> Result<(), ApiError> {
        self.publish_fact(|hub_seq| HubStreamEvent::Fact {
            hub_seq,
            node,
            id,
            session_id,
            timestamp,
            event,
        })
    }

    /// 转发增量。仅实时，不进回放窗口。
    pub fn delta(
        &self,
        node: NodeName,
        session_id: SessionId,
        message_id: durable::MessageId,
        text: String,
        timestamp: i64,
    ) {
        self.broadcast(HubStreamEvent::Delta {
            node,
            session_id,
            message_id,
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

fn seq_of(event: &HubStreamEvent) -> u64 {
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

/// 为单个节点维持常驻上游事件流，带指数退避重连。
pub(crate) struct NodeWatcher {
    handle: JoinHandle<()>,
}

impl NodeWatcher {
    pub(crate) fn start(
        bus: Arc<Bus>,
        node: NodeMetadata,
        client: reqwest::Client,
        options: HubOptions,
    ) -> Self {
        let handle = tokio::spawn(async move { watch_forever(bus, node, client, options).await });
        Self { handle }
    }
}

impl Drop for NodeWatcher {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn watch_forever(
    bus: Arc<Bus>,
    node: NodeMetadata,
    client: reqwest::Client,
    options: HubOptions,
) {
    match watch_loop(&bus, &node, &client, options).await {
        Ok(never) => match never {},
        Err(error) => {
            tracing::error!(node = %node.name, error = %error, "总线状态不可用，停止该节点的上游订阅");
        }
    }
}

/// 常驻订阅循环：上游每结束一次就按退避重连。总线状态不可用时返回错误。
async fn watch_loop(
    bus: &Bus,
    node: &NodeMetadata,
    client: &reqwest::Client,
    options: HubOptions,
) -> Result<Infallible, ApiError> {
    let mut last_id: Option<i64> = None;
    let mut attempt = 0u32;
    loop {
        let was_ok = bus.view(&node.name)?.status == NodeStatus::Ok;
        let end = match run_stream(bus, client, node, &mut last_id, options).await {
            Ok(never) => match never {},
            Err(WatchError::End(end)) => end,
            Err(WatchError::Bus(error)) => return Err(error),
        };
        if let StreamEnd::Failure(failure) = &end {
            bus.set_view(
                node.name.clone(),
                NodeView {
                    status: failure.status,
                    detail: Some(failure.detail.clone()),
                },
            )?;
        }
        if let StreamEnd::Resync(reason) = &end {
            tracing::debug!(node = %node.name, ?reason, "上游要求重连");
        }
        attempt = next_attempt(&end, was_ok, attempt);
        tokio::time::sleep(backoff_delay(&node.name, attempt, options.backoff_base)).await;
    }
}

/// 下一次重连的退避序号。
///
/// 失败按次数累加；刚恢复过的节点从第一次重来；上游要求 resync 不算失败，也因此回到最短等待。
fn next_attempt(end: &StreamEnd, was_ok: bool, attempt: u32) -> u32 {
    match end {
        StreamEnd::Failure(_) if was_ok => 1,
        StreamEnd::Failure(_) => attempt.saturating_add(1),
        StreamEnd::Resync(_) => 1,
    }
}

/// 重连退避：底数翻倍，抖动 ±20%，抖动之后仍不超过上限。
///
/// 抖动由 `(节点名, 尝试次数)` 决定：确定性让退避序列可被单测直接断言，
/// 同时把同时重连的节点错开。上限取 `底数 × 8 ÷ 1.2`，所以抖动后最长等待为 `底数 × 8`。
fn backoff_delay(node: &NodeName, attempt: u32, base: Duration) -> Duration {
    let ceiling = base.mul_f64(8.0 / 1.2);
    let exponent = attempt.saturating_sub(1).min(3);
    let nominal = base.saturating_mul(1 << exponent).min(ceiling);
    nominal.mul_f64(jitter(node, attempt))
}

/// 抖动系数，值域 `[0.8, 1.2]`。
fn jitter(node: &NodeName, attempt: u32) -> f64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in node.as_str().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash ^= u64::from(attempt);
    hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    #[expect(
        clippy::cast_precision_loss,
        reason = "取模 1001 之后小于 1001，f64 可精确表示。"
    )]
    let permille = (hash % 1001) as f64;
    0.8 + 0.4 * permille / 1000.0
}

/// 一次上游订阅的结束原因。
///
/// `Failure` 写节点状态；`Resync` 只触发重连。节点要求 resync 时仍然健康，只是 hub 落后于
/// agent 的内存广播容量。把它当成失败会短暂禁用该节点的全部命令，并让真正的不可达原因与
/// 「hub 落后」混在同一条说明里（ADR 0030）。
///
/// `Resync` 不动 `last_id`：下一次订阅从最后消费的事实续传，agent 从 SQLite 补齐（ADR 0027）。
enum StreamEnd {
    Failure(Failure),
    Resync(ResyncReason),
}

impl From<StreamEnd> for WatchError {
    fn from(end: StreamEnd) -> Self {
        Self::End(end)
    }
}

impl From<Failure> for WatchError {
    fn from(failure: Failure) -> Self {
        Self::End(StreamEnd::Failure(failure))
    }
}

impl From<ApiError> for WatchError {
    fn from(error: ApiError) -> Self {
        Self::Bus(error)
    }
}

/// 订阅循环的错误：要么一次上游订阅结束，要么总线状态不可用。
enum WatchError {
    End(StreamEnd),
    Bus(ApiError),
}

/// 上游订阅只以错误结束：上游关闭事件流也按一次失败处理。
type StreamResult = Result<Infallible, WatchError>;

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
    bus: &Bus,
    client: &reqwest::Client,
    node: &NodeMetadata,
    last_id: &mut Option<i64>,
    options: HubOptions,
) -> StreamResult {
    let base = node_probe::base_url(node);
    let name = &node.name;
    let health = client
        .get(format!("{base}/api/health"))
        .timeout(options.connect_timeout)
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
        ))
        .into());
    }
    bus.set_view(
        name.clone(),
        NodeView {
            status: NodeStatus::Ok,
            detail: None,
        },
    )?;

    // 水位探测失败必须降级，不能退回 after=0 继续订阅：那会把节点的全部历史重新灌进
    // 合并流，正是本决策要消除的行为（ADR 0027）。拿不到水位就意味着拿不到该节点的快照，
    // 此时让它保持不可达，比让客户端收到一份没有快照支撑的事实流更诚实。
    let after = if let Some(id) = *last_id {
        id
    } else {
        let bootstrap = node_probe::fetch_bootstrap(client, node, options.connect_timeout)
            .await
            .map_err(|detail| Failure::unreachable(format!("读取节点水位失败: {detail}")))?;
        *last_id = Some(bootstrap.max_event_id);
        bootstrap.max_event_id
    };
    // 只给「等响应头」加时限。请求级 timeout 会覆盖到响应体读完，从而杀死整条常驻事件流。
    let response = tokio::time::timeout(
        options.connect_timeout,
        client
            .get(format!("{base}/api/events?after={after}"))
            .send(),
    )
    .await
    .map_err(|_| Failure::unreachable("等待事件流响应超时"))?
    .map_err(|error| Failure::unreachable(format!("订阅事件流失败: {error}")))?;
    if !response.status().is_success() {
        return Err(Failure::unreachable(format!("事件流返回 {}", response.status())).into());
    }
    read_events(
        response.bytes_stream(),
        name,
        last_id,
        bus,
        options.stream_idle_timeout,
    )
    .await
}

/// 逐块读取事件流：按空行切帧，把每帧的 `data:` 行交给 [`dispatch`]。
async fn read_events(
    stream: impl futures_util::Stream<Item = reqwest::Result<Bytes>>,
    name: &NodeName,
    last_id: &mut Option<i64>,
    bus: &Bus,
    idle: Duration,
) -> StreamResult {
    let mut stream = Box::pin(stream);
    let mut buffer: Vec<u8> = Vec::new();
    loop {
        let chunk = match tokio::time::timeout(idle, stream.next()).await {
            Ok(Some(Ok(chunk))) => chunk,
            Ok(Some(Err(error))) => {
                return Err(Failure::unreachable(format!("读取事件流失败: {error}")).into());
            }
            Ok(None) => return Err(Failure::unreachable("事件流结束").into()),
            Err(_) => return Err(Failure::unreachable("事件流空闲超时").into()),
        };
        buffer.extend_from_slice(&chunk);
        while let Some(frame) = take_frame(&mut buffer) {
            let text = String::from_utf8_lossy(&frame);
            for line in text.lines() {
                let Some(payload) = line.strip_prefix("data: ") else {
                    continue;
                };
                let Ok(event) = serde_json::from_str::<StreamEvent>(payload) else {
                    continue;
                };
                dispatch(event, name, last_id, bus)?;
            }
        }
    }
}

/// 从缓冲区切出一帧（到空行为止）。
fn take_frame(buffer: &mut Vec<u8>) -> Option<Vec<u8>> {
    let end = buffer.windows(2).position(|window| window == b"\n\n")?;
    Some(buffer.drain(..end + 2).collect())
}

/// 把一条上游事件交给总线。`Resync` 结束本次订阅，由上层重连。
fn dispatch(
    event: StreamEvent,
    name: &NodeName,
    last_id: &mut Option<i64>,
    bus: &Bus,
) -> Result<(), WatchError> {
    match event {
        StreamEvent::Fact {
            id,
            session_id,
            timestamp,
            event,
        } => {
            *last_id = Some(id);
            bus.node_fact(name.clone(), id, session_id, timestamp, event)?;
        }
        StreamEvent::Delta {
            session_id,
            message_id,
            text,
            timestamp,
        } => bus.delta(name.clone(), session_id, message_id, text, timestamp),
        StreamEvent::Retry {
            session_id,
            attempt,
            max_attempts,
            delay_ms,
        } => bus.retry(name.clone(), session_id, attempt, max_attempts, delay_ms),
        StreamEvent::Resync { reason } => return Err(StreamEnd::Resync(reason).into()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(raw: &str) -> NodeName {
        NodeName::parse(raw).expect("合法节点名")
    }

    fn view(status: NodeStatus, detail: &str) -> NodeView {
        NodeView {
            status,
            detail: Some(detail.to_owned()),
        }
    }

    /// 退避序列：底数翻倍、抖动 ±20%、抖动之后仍不超过上限。
    #[test]
    fn backoff_delay_stays_within_jittered_bounds() {
        let base = Duration::from_millis(100);
        let ceiling = 800.0;
        for index in 0..64 {
            let node = name(&format!("node-{index}"));
            for attempt in 1..=10 {
                let elapsed = backoff_delay(&node, attempt, base).as_secs_f64() * 1000.0;
                let nominal = match attempt {
                    1 => 100.0,
                    2 => 200.0,
                    3 => 400.0,
                    _ => ceiling / 1.2,
                };
                assert!(
                    elapsed <= ceiling + 0.001,
                    "node-{index} 第 {attempt} 次退避 {elapsed} 毫秒超过上限"
                );
                assert!(
                    elapsed >= nominal * 0.8 - 0.001 && elapsed <= nominal * 1.2 + 0.001,
                    "node-{index} 第 {attempt} 次退避 {elapsed} 毫秒超出抖动区间"
                );
            }
        }
    }

    /// 抖动把同时重连的节点错开。
    #[test]
    fn jitter_differs_between_nodes() {
        let base = Duration::from_secs(1);
        assert_ne!(
            backoff_delay(&name("laptop"), 4, base),
            backoff_delay(&name("server"), 4, base)
        );
    }

    /// 同一节点同一尝试次数总是同一个等待，因此可以被断言。
    #[test]
    fn backoff_delay_is_deterministic() {
        let base = Duration::from_millis(100);
        assert_eq!(
            backoff_delay(&name("laptop"), 3, base),
            backoff_delay(&name("laptop"), 3, base)
        );
    }

    /// 上游要求 resync 不是失败：重连走最短等待，不按失败次数翻倍。
    #[test]
    fn resync_restarts_backoff_at_shortest_wait() {
        let base = Duration::from_millis(100);
        let laptop = name("laptop");
        let resync = StreamEnd::Resync(ResyncReason::Lagged { skipped: 3 });
        let failure = StreamEnd::Failure(Failure::unreachable("读取事件流失败"));

        let after_resync = next_attempt(&resync, false, 5);
        assert_eq!(
            backoff_delay(&laptop, after_resync, base),
            backoff_delay(&laptop, 1, base),
            "resync 后的等待必须与第一次重连相同"
        );
        assert_eq!(next_attempt(&failure, false, 5), 6, "持续失败才累加");
        assert_eq!(
            next_attempt(&failure, true, 5),
            1,
            "刚恢复过的节点从第一次重来"
        );
    }

    /// 状态说明变化也广播；连续相同的组合不发。
    #[test]
    fn set_view_broadcasts_detail_changes_once() {
        let bus = Bus::new(16, 16);
        let mut receiver = bus.sender.subscribe();
        let laptop = name("laptop");

        bus.set_view(
            laptop.clone(),
            view(NodeStatus::Unreachable, "连接节点失败"),
        )
        .expect("写入视图");
        bus.set_view(
            laptop.clone(),
            view(NodeStatus::Unreachable, "连接节点失败"),
        )
        .expect("写入视图");
        bus.set_view(laptop.clone(), view(NodeStatus::Unreachable, "事件流结束"))
            .expect("写入视图");

        assert_eq!(
            bus.max_seq().expect("取最大序号"),
            2,
            "相同的 (status, detail) 只广播一次"
        );

        let mut details = Vec::new();
        while let Ok(HubStreamEvent::NodeStatusChanged { detail, .. }) = receiver.try_recv() {
            details.push(detail);
        }
        assert_eq!(
            details,
            vec![
                Some("连接节点失败".to_owned()),
                Some("事件流结束".to_owned())
            ]
        );
    }
}
