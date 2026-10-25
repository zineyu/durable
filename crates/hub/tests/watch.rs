#![expect(
    clippy::panic,
    reason = "测试脚手架用 panic 直接报告断言失败：事件类型不符必须立刻暴露。"
)]

mod support;

use std::time::Duration;

use support::fixtures::EventStream;
use support::fixtures::scripted::{
    EventsMode, ScriptedNode, fact, wait_for_after, wait_for_upstream,
};
use support::harness::Harness;

fn options() -> hub::HubOptions {
    hub::HubOptions {
        broadcast_capacity: 64,
        replay_window: 64,
        connect_timeout: Duration::from_millis(200),
        command_timeout: Duration::from_millis(300),
        stream_idle_timeout: Duration::from_secs(5),
        backoff_base: Duration::from_millis(20),
    }
}

fn fact_id(event: &hub::HubStreamEvent) -> i64 {
    match event {
        hub::HubStreamEvent::Fact { id, .. } => *id,
        other => panic!("不是事实事件: {other:?}"),
    }
}

/// 不可达事件的说明。可达状态的广播不算原因变化。
fn unreachable_detail(event: &hub::HubStreamEvent) -> Option<&str> {
    match event {
        hub::HubStreamEvent::NodeStatusChanged {
            status: hub::NodeStatus::Unreachable,
            detail,
            ..
        } => Some(detail.as_deref().unwrap_or("")),
        _ => None,
    }
}

/// 规格 user story H-12：状态保持不可达而原因变化时，已订阅的客户端看到新的说明。
///
/// 这条路径只在健康检查失败时出现：任何通过的尝试都会先把状态置为 ok，
/// 状态一变就会广播，因此只有两次相邻的 unreachable 才需要靠说明区分。
#[tokio::test]
async fn broadcasts_detail_change_while_status_stays_unreachable() {
    let harness = Harness::start_with_options(options()).await;
    let node = ScriptedNode::start().await;
    node.set_health_body("不是 JSON");
    harness.register_node("laptop", &node.url).await;

    let first = harness
        .wait_for_status("laptop", hub::NodeStatus::Unreachable)
        .await;
    let first_detail = first.detail.expect("不可达必须带说明");
    assert!(
        first_detail.contains("health 响应无法解析"),
        "首个原因应是健康检查无法解析: {first_detail}"
    );

    let cursor = harness.bootstrap().await.max_hub_seq;
    let mut stream = EventStream::connect(&harness, cursor).await;

    node.stop();
    node.wait_until_refused().await;
    stream
        .next_matching(|event| {
            unreachable_detail(event).is_some_and(|detail| detail.contains("连接节点失败"))
        })
        .await;
}

/// 探测读得到响应头却读不到 body 时，水位读取必须按时限失败，不能无限挂起。
#[tokio::test]
async fn demotes_node_when_probe_body_stalls() {
    let harness = Harness::start_with_options(options()).await;
    let node = ScriptedNode::start().await;
    node.stall_bootstrap_body();
    harness.register_node("laptop", &node.url).await;
    let started = std::time::Instant::now();

    let cursor = harness.bootstrap().await.max_hub_seq;
    let mut stream = EventStream::connect(&harness, cursor).await;
    stream
        .next_matching(|event| {
            unreachable_detail(event).is_some_and(|detail| detail.contains("读取节点水位失败"))
        })
        .await;
    // 时限来自 fetch_bootstrap 的请求级 timeout，connect_timeout 同时是探测的总时限。
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "探测必须在时限内失败，实际 {:?}",
        started.elapsed()
    );
}

/// 上游接受连接却永不返回响应头时，订阅必须按时限失败。
#[tokio::test]
async fn demotes_node_when_stream_headers_never_arrive() {
    let harness = Harness::start_with_options(options()).await;
    let node = ScriptedNode::start().await;
    node.set_events_mode(EventsMode::Stall);
    harness.register_node("laptop", &node.url).await;
    let started = std::time::Instant::now();

    let cursor = harness.bootstrap().await.max_hub_seq;
    let mut stream = EventStream::connect(&harness, cursor).await;
    stream
        .next_matching(|event| {
            unreachable_detail(event).is_some_and(|detail| detail.contains("等待事件流响应超时"))
        })
        .await;
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "等响应头必须在时限内失败，实际 {:?}",
        started.elapsed()
    );
}

/// ADR 0027：hub 重启后上游从节点水位起订阅，不重播历史事实。
#[tokio::test]
async fn restarts_upstream_from_node_watermark() {
    let harness = Harness::start_with_options(options()).await;
    let node = ScriptedNode::start().await;
    node.set_watermark(500);
    harness.register_node("laptop", &node.url).await;
    harness.wait_for_status("laptop", hub::NodeStatus::Ok).await;
    wait_for_upstream(&node).await;

    assert_eq!(
        node.after_requests().last().copied().flatten(),
        Some(500),
        "首次订阅必须使用节点水位而不是 0"
    );

    node.set_watermark(700);
    let directory = harness.stop().await;
    let harness = Harness::start_options(directory, options()).await;
    wait_for_after(&node, 700).await;

    let cursor = harness.bootstrap().await.max_hub_seq;
    let mut stream = EventStream::connect(&harness, cursor).await;
    node.publish_fact(700, fact());
    let event = stream
        .next_matching(|event| matches!(event, hub::HubStreamEvent::Fact { .. }))
        .await;
    assert_eq!(fact_id(&event), 700, "重启后不应重播历史事实");
}

/// 上游安静超过空闲超时后节点被降级，说明点明空闲超时。
#[tokio::test]
async fn demotes_node_when_upstream_stream_goes_idle() {
    let harness = Harness::start_with_options(hub::HubOptions {
        stream_idle_timeout: Duration::from_millis(200),
        ..options()
    })
    .await;
    let node = ScriptedNode::start().await;
    harness.register_node("laptop", &node.url).await;
    harness.wait_for_status("laptop", hub::NodeStatus::Ok).await;
    wait_for_upstream(&node).await;

    let cursor = harness.bootstrap().await.max_hub_seq;
    let mut stream = EventStream::connect(&harness, cursor).await;
    stream
        .next_matching(|event| {
            unreachable_detail(event).is_some_and(|detail| detail.contains("事件流空闲超时"))
        })
        .await;
}

/// ADR 0030：hub 落后于上游广播时只重连，不把节点当成不可达。
///
/// 落后的游标不动，重连从最后消费的事实续传，因此回放之后事实齐全。
#[tokio::test]
async fn resync_reconnects_without_marking_node_unreachable() {
    let harness = Harness::start_with_options(hub::HubOptions {
        // 客户端自己不能因为总线容量不足而收到 resync，否则本用例观察的就不是上游行为了。
        broadcast_capacity: 4096,
        replay_window: 4096,
        ..options()
    })
    .await;
    let node = ScriptedNode::start_with_fact_capacity(1).await;
    harness.register_node("laptop", &node.url).await;
    harness.wait_for_status("laptop", hub::NodeStatus::Ok).await;
    wait_for_upstream(&node).await;

    let cursor = harness.bootstrap().await.max_hub_seq;
    let mut stream = EventStream::connect(&harness, cursor).await;

    let total = 200;
    for id in 1..=total {
        node.publish_fact(id, fact());
    }

    let mut seen = Vec::new();
    while seen.last().copied() != Some(total) {
        let event = stream.next_event().await;
        assert!(
            !matches!(event, hub::HubStreamEvent::NodeStatusChanged { .. }),
            "上游 resync 不得改变节点状态: {event:?}"
        );
        if let hub::HubStreamEvent::Fact { id, .. } = event {
            seen.push(id);
        }
    }
    assert_eq!(
        seen,
        (1..=total).collect::<Vec<_>>(),
        "重连必须从最后消费的事实续传，补齐落后期间的事实"
    );
    assert_eq!(
        harness.snapshot("laptop").await.status,
        hub::NodeStatus::Ok,
        "resync 之后节点仍是可用"
    );
}

/// 退避使用注入的底数：卡住的节点在一秒内多次重连，不会退到几十秒。
#[tokio::test]
async fn reconnects_quickly_with_small_backoff_base() {
    let harness = Harness::start_with_options(options()).await;
    let node = ScriptedNode::start().await;
    node.set_events_mode(EventsMode::End);
    harness.register_node("laptop", &node.url).await;
    harness
        .wait_for_status("laptop", hub::NodeStatus::Unreachable)
        .await;

    let before = node.after_requests().len();
    tokio::time::sleep(Duration::from_secs(1)).await;
    let attempts = node.after_requests().len() - before;
    assert!(
        (3..=40).contains(&attempts),
        "一秒内应多次重连，但不应退到几十秒一次，实际 {attempts}"
    );
}

/// 规格 user story H-57 与 H-58：快照带最后事件时间与 health 往返延迟。
#[tokio::test]
async fn reports_last_event_time_and_health_latency() {
    let harness = Harness::start_with_options(options()).await;
    let node = ScriptedNode::start().await;
    harness.register_node("laptop", &node.url).await;
    let available = harness.wait_for_status("laptop", hub::NodeStatus::Ok).await;
    assert_eq!(available.last_event_at, None, "尚未收到事实事件");
    assert!(available.latency_ms.is_some(), "health 往返应已记录");
    wait_for_upstream(&node).await;

    node.publish_fact(1000, fact());
    let read = tokio::time::Instant::now();
    loop {
        let snapshot = harness.snapshot("laptop").await;
        if snapshot.last_event_at.is_some() {
            assert_eq!(
                snapshot.last_event_at,
                Some(1000),
                "记录收到的最后一条事实自带的时间戳"
            );
            break;
        }
        assert!(
            read.elapsed() < Duration::from_secs(5),
            "最后事件时间未在时限内出现"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        harness.snapshot("laptop").await.last_event_at,
        Some(1000),
        "没有新事实时该值不变"
    );
}
