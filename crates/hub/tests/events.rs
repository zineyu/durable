mod support;

use support::scripted::{ScriptedNode, fact, wait_for_upstream};
use support::{AgentHandle, EventStream, Harness};

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
    reqwest::Client::new()
        .post(harness.url(&format!("/api/n/{node}/workspaces")))
        .json(&serde_json::json!({ "path": path }))
        .send()
        .await
        .expect("经 hub 注册 workspace");
}

/// 规格 user story 30：合并流的广播序号跨节点严格递增。
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

/// 规格 user story 31：游标落在回放窗口内时回放缺失事件。
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

/// 规格 user story 33：游标落在窗口外时返回 resync。
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

/// 规格 user story 34：游标超前于当前序号（例如 hub 重启后）时要求 resync。
#[tokio::test]
async fn resyncs_when_cursor_is_ahead_of_hub() {
    let harness = Harness::start().await;

    let mut stream = EventStream::connect(&harness, 1_000_000).await;
    assert!(is_resync(&stream.next_event().await));
}

/// 规格 user story 1：快照进行中产生的事实不会漏掉。
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

fn seq_of(event: &hub::HubStreamEvent) -> u64 {
    match event {
        hub::HubStreamEvent::Fact { hub_seq, .. }
        | hub::HubStreamEvent::NodeRegistered { hub_seq, .. }
        | hub::HubStreamEvent::NodeRemoved { hub_seq, .. }
        | hub::HubStreamEvent::NodeStatusChanged { hub_seq, .. } => *hub_seq,
        hub::HubStreamEvent::Delta { .. } | hub::HubStreamEvent::Resync { .. } => 0,
    }
}

fn node_of(event: &hub::HubStreamEvent) -> String {
    match event {
        hub::HubStreamEvent::Fact { node, .. } => node.as_str().to_owned(),
        other => panic!("不是节点事实事件: {other:?}"),
    }
}
