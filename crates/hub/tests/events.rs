#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "测试脚手架用 expect 与 panic 直接报告夹具与断言失败：工作目录创建失败必须立刻暴露。"
)]

mod support;

use support::fixtures::EventStream;
use support::fixtures::scripted::{ScriptedNode, fact, wait_for_upstream};
use support::harness::{AgentHandle, Harness};

fn is_fact(event: &hub::HubStreamEvent) -> bool {
    matches!(event, hub::HubStreamEvent::Fact { .. })
}

fn is_resync(event: &hub::HubStreamEvent) -> bool {
    matches!(event, hub::HubStreamEvent::Resync { .. })
}

fn workspaces(directory: &std::path::Path, count: usize) -> Vec<String> {
    (0..count)
        .map(|index| {
            let path = directory.join(format!("ws-{index}"));
            std::fs::create_dir(&path).expect("创建 workspace 子目录");
            path.to_str().expect("临时路径是 UTF-8").to_owned()
        })
        .collect()
}

async fn register_workspace(harness: &Harness, node: &str, path: &str) {
    let response = reqwest::Client::new()
        .post(harness.url(&format!("/api/n/{node}/workspaces")))
        .json(&serde_json::json!({ "host": "local", "path": path }))
        .send()
        .await
        .expect("经 hub 注册 workspace");
    assert!(
        response.status().is_success(),
        "注册 workspace 必须成功: {} {}",
        response.status(),
        response.text().await.unwrap_or_default()
    );
}

/// 规格 user story H-30：合并流的广播序号跨节点严格递增。
#[tokio::test]
async fn broadcasts_ordered_hub_seq_across_nodes() {
    let harness = Harness::start().await;
    let first = AgentHandle::start().await;
    let second = AgentHandle::start().await;
    harness.register_node("first", &first.url).await;
    harness.register_node("second", &second.url).await;
    harness.wait_for_status("first", hub::NodeStatus::Ok).await;
    harness.wait_for_status("second", hub::NodeStatus::Ok).await;

    let cursor = harness.bootstrap().await.max_hub_seq;
    let mut stream = EventStream::connect(&harness, cursor).await;

    for (node, agent) in [("first", &first), ("second", &second)] {
        let path = workspaces(agent.directory.path(), 1);
        register_workspace(&harness, node, &path[0]).await;
    }

    let first_event = stream.next_matching(is_fact).await;
    let second_event = stream.next_matching(is_fact).await;
    assert!(
        seq_of(&second_event) > seq_of(&first_event),
        "广播序号必须递增"
    );
    assert_eq!(node_of(&first_event), "first", "事件必须标明来源节点");
    assert_eq!(node_of(&second_event), "second");
}

/// 规格 user story H-31：游标落在回放窗口内时回放缺失事件。
#[tokio::test]
async fn replays_events_after_cursor_within_window() {
    let harness = Harness::start().await;
    let laptop = AgentHandle::start().await;
    harness.register_node("laptop", &laptop.url).await;
    harness.wait_for_status("laptop", hub::NodeStatus::Ok).await;

    let cursor = harness.bootstrap().await.max_hub_seq;
    let path = workspaces(laptop.directory.path(), 1);
    register_workspace(&harness, "laptop", &path[0]).await;
    harness.wait_for_seq_after(cursor).await;

    let mut stream = EventStream::connect(&harness, cursor).await;
    let event = stream.next_matching(is_fact).await;
    assert_eq!(
        seq_of(&event),
        cursor + 1,
        "回放应从游标之后的第一个事件开始"
    );
}

/// 规格 user story H-33：游标落在窗口外时返回 resync。
#[tokio::test]
async fn resyncs_when_cursor_falls_outside_window() {
    let harness = Harness::start_with_options(hub::HubOptions {
        broadcast_capacity: 64,
        replay_window: 2,
        ..hub::HubOptions::default()
    })
    .await;
    let laptop = AgentHandle::start().await;
    harness.register_node("laptop", &laptop.url).await;
    harness.wait_for_status("laptop", hub::NodeStatus::Ok).await;

    let baseline = harness.bootstrap().await.max_hub_seq;
    for path in workspaces(laptop.directory.path(), 3) {
        register_workspace(&harness, "laptop", &path).await;
    }
    harness.wait_for_seq_after(baseline + 2).await;

    let mut stream = EventStream::connect(&harness, 0).await;
    assert!(
        is_resync(&stream.next_event().await),
        "窗口外游标应要求 resync"
    );
}

/// 规格 user story H-34：游标超前于当前序号（例如 hub 重启后）时要求 resync。
#[tokio::test]
async fn resyncs_when_cursor_is_ahead_of_hub() {
    let harness = Harness::start().await;

    let mut stream = EventStream::connect(&harness, 1_000_000).await;
    assert!(is_resync(&stream.next_event().await));
}

/// ADR 0029：快照进行中产生的事实不会漏掉。
///
/// 水位必须在任何节点请求之前取：否则该事实既不在快照里，也不在游标之后。
#[tokio::test]
async fn bootstrap_watermark_precedes_node_requests() {
    let harness = Harness::start().await;
    let node = ScriptedNode::start().await;
    node.set_watermark(10);
    harness.register_node("laptop", &node.url).await;
    harness.wait_for_status("laptop", hub::NodeStatus::Ok).await;
    wait_for_upstream(&node).await;

    let baseline = harness.bootstrap().await.max_hub_seq;
    node.hold_bootstrap();
    let pending = tokio::spawn({
        let url = harness.url("/api/bootstrap");
        async move {
            reqwest::get(url)
                .await
                .expect("请求 bootstrap")
                .json::<hub::HubBootstrap>()
                .await
                .expect("解析 bootstrap")
        }
    });
    node.wait_for_bootstrap_request().await;

    let mut probe = EventStream::connect(&harness, baseline).await;
    node.publish_fact(4242, fact());
    let event = probe.next_matching(is_fact).await;
    let fact_seq = seq_of(&event);
    node.release_bootstrap();

    let bootstrap = pending.await.expect("等待 bootstrap 完成");
    assert!(
        bootstrap.max_hub_seq < fact_seq,
        "水位必须早于快照期间的事实: {} 不小于 {fact_seq}",
        bootstrap.max_hub_seq
    );

    let mut client = EventStream::connect(&harness, bootstrap.max_hub_seq).await;
    let replayed = client
        .next_matching(|event| matches!(event, hub::HubStreamEvent::Fact { id: 4242, .. }))
        .await;
    assert_eq!(
        seq_of(&replayed),
        fact_seq,
        "快照之后必须回放快照期间的事实"
    );
}

/// 事实信封的 session 视图随合并流原样到达客户端。
#[tokio::test]
async fn forwards_session_view_of_fact_envelope() {
    let harness = Harness::start().await;
    let node = ScriptedNode::start().await;
    harness.register_node("laptop", &node.url).await;
    harness.wait_for_status("laptop", hub::NodeStatus::Ok).await;
    wait_for_upstream(&node).await;

    let before = harness.bootstrap().await.max_hub_seq;
    let view = durable::SessionMetadata {
        id: durable::SessionId::parse("s1").expect("合法 session id"),
        host: durable::HostName::parse("local").expect("合法 host 名"),
        workspace: durable::WorkspacePath::parse("/work").expect("合法 workspace 路径"),
        title: durable::SessionTitle::parse("标题").expect("合法标题"),
        busy: true,
        queue_length: 2,
        updated_at: 7,
    };
    node.publish_fact_with_view(9, view.clone(), fact());
    harness.wait_for_seq_after(before).await;

    let mut stream = EventStream::connect(&harness, before).await;
    let event = stream.next_matching(is_fact).await;
    let hub::HubStreamEvent::Fact { session, .. } = event else {
        panic!("不是事实事件");
    };
    assert_eq!(session, Some(view), "信封的 session 视图必须原样转发");
}

fn seq_of(event: &hub::HubStreamEvent) -> u64 {
    match event {
        hub::HubStreamEvent::Fact { hub_seq, .. }
        | hub::HubStreamEvent::NodeRegistered { hub_seq, .. }
        | hub::HubStreamEvent::NodeRemoved { hub_seq, .. }
        | hub::HubStreamEvent::NodeUpdated { hub_seq, .. }
        | hub::HubStreamEvent::NodeStatusChanged { hub_seq, .. } => *hub_seq,
        hub::HubStreamEvent::Delta { .. }
        | hub::HubStreamEvent::Retry { .. }
        | hub::HubStreamEvent::Resync { .. } => 0,
    }
}

fn node_of(event: &hub::HubStreamEvent) -> String {
    match event {
        hub::HubStreamEvent::Fact { node, .. } => node.as_str().to_owned(),
        other => panic!("不是节点事实事件: {other:?}"),
    }
}

/// ADR 0031：重试提示随合并流到达，带节点名且不推进广播序号。
#[tokio::test]
async fn forwards_retry_hint_without_advancing_cursor() {
    let harness = Harness::start().await;
    let node = ScriptedNode::start().await;
    harness.register_node("laptop", &node.url).await;
    harness.wait_for_status("laptop", hub::NodeStatus::Ok).await;
    wait_for_upstream(&node).await;

    let cursor = harness.bootstrap().await.max_hub_seq;
    let mut stream = EventStream::connect(&harness, cursor).await;
    node.publish_retry("s1", 2, 3, 4000);

    let event = stream
        .next_matching(|event| matches!(event, hub::HubStreamEvent::Retry { .. }))
        .await;
    match event {
        hub::HubStreamEvent::Retry {
            node,
            session_id,
            attempt,
            max_attempts,
            delay_ms,
        } => {
            assert_eq!(node.as_str(), "laptop");
            assert_eq!(session_id.as_str(), "s1");
            assert_eq!((attempt, max_attempts, delay_ms), (2, 3, 4000));
        }
        other => panic!("不是重试提示: {other:?}"),
    }
    assert_eq!(
        harness.bootstrap().await.max_hub_seq,
        cursor,
        "重试提示不推进广播序号"
    );
}

/// 规格 user story H-59：回放窗口容量为 1 时，窗口内游标回放，窗口外游标收到 resync。
#[tokio::test]
async fn honours_single_event_replay_window() {
    let harness = Harness::start_with_options(hub::HubOptions {
        replay_window: 1,
        broadcast_capacity: 64,
        ..hub::HubOptions::default()
    })
    .await;
    let node = ScriptedNode::start().await;
    harness.register_node("laptop", &node.url).await;
    harness.wait_for_status("laptop", hub::NodeStatus::Ok).await;
    wait_for_upstream(&node).await;

    // 以当前序号为基准，避免把此前节点的登记与状态事件误当成第一条事实。
    let before = harness.bootstrap().await.max_hub_seq;
    node.publish_fact(1, fact());
    let first = harness.wait_for_seq_after(before).await;
    node.publish_fact(2, fact());
    let second = harness.wait_for_seq_after(first).await;

    let mut inside = EventStream::connect(&harness, first).await;
    let replayed = inside.next_matching(is_fact).await;
    match replayed {
        hub::HubStreamEvent::Fact { id, .. } => assert_eq!(id, 2),
        other => panic!("不是事实事件: {other:?}"),
    }

    let mut outside = EventStream::connect(&harness, second.saturating_sub(2)).await;
    let event = outside.next_event().await;
    assert!(
        matches!(event, hub::HubStreamEvent::Resync { .. }),
        "窗口外游标必须收到 resync: {event:?}"
    );
}
