//! 单个节点的常驻上游订阅：健康探测、水位起点、逐帧分发与指数退避重连。
//! 事件如何进入总线在父模块；resync 语义见 ADR 0030。

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use agent::HealthResponse;
use axum::body::Bytes;
use durable::{NodeMetadata, NodeName, ResyncReason, StreamEvent};
use futures_util::StreamExt;
use tokio::task::JoinHandle;

use super::Bus;
use super::node::NodeView;
use crate::node_probe;
use crate::protocol::{NodeStatus, REQUIRED_AGENT_PROTOCOL_VERSION};
use crate::{ApiError, HubOptions};

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
        epoch: u64,
    ) -> Self {
        let handle =
            tokio::spawn(async move { watch_forever(bus, node, client, options, epoch).await });
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
    epoch: u64,
) {
    match watch_loop(&bus, &node, &client, options, epoch).await {
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
    epoch: u64,
) -> Result<Infallible, ApiError> {
    let mut last_id: Option<i64> = None;
    let mut attempt = 0u32;
    loop {
        let was_ok = bus.node_entry(&node.name)?.view.status == NodeStatus::Ok;
        let end = match run_stream(bus, client, node, &mut last_id, options, epoch).await {
            Ok(never) => match never {},
            Err(WatchError::End(end)) => end,
            Err(WatchError::Bus(error)) => return Err(error),
        };
        if let StreamEnd::Failure(failure) = &end {
            bus.set_probe_view(
                node.name.clone(),
                epoch,
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
    epoch: u64,
) -> StreamResult {
    let base = node_probe::base_url(node);
    let name = &node.name;
    let started = tokio::time::Instant::now();
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
    let latency_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    bus.record_latency(name, latency_ms)?;
    if health.protocol_version != REQUIRED_AGENT_PROTOCOL_VERSION {
        return Err(Failure::incompatible(format!(
            "agent 协议版本 {} 与 hub 要求的 {} 不一致",
            health.protocol_version, REQUIRED_AGENT_PROTOCOL_VERSION
        ))
        .into());
    }
    bus.set_probe_view(
        name.clone(),
        epoch,
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
            session,
            timestamp,
            event,
        } => {
            *last_id = Some(id);
            bus.node_fact(name.clone(), id, session_id, session, timestamp, event)?;
        }
        StreamEvent::Delta {
            session_id,
            message_id,
            kind,
            text,
            timestamp,
        } => bus.delta(name.clone(), session_id, message_id, kind, text, timestamp),
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
}
