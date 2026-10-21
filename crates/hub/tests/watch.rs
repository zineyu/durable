mod support;

use support::scripted::ScriptedNode;
use support::{EventStream, Harness};

fn options() -> hub::HubOptions {
    hub::HubOptions {
        broadcast_capacity: 64,
        replay_window: 64,
        ..hub::HubOptions::default()
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

/// 规格 user story 12：状态保持不可达而原因变化时，已订阅的客户端看到新的说明。
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
