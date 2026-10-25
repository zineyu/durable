//! 插话（`mode=steer`）的集成测试：步骤边界生效、工具不打断、降级与顺序。
//! 排队语义与自动标题在 `queue`，run 生命周期在 `run`。

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "测试辅助函数用 expect、下标与 panic 直接报告夹具与断言失败。"
)]

use std::time::Duration;

use reqwest::StatusCode;

mod support;

use support::{Harness, Script, error_code, recorded_events};

fn queue_texts(tree: &agent::SessionTree) -> Vec<&str> {
    tree.queue.iter().map(|item| item.text.as_str()).collect()
}

/// 指定角色的全部消息正文，按消息 id 升序。
async fn role_texts(
    harness: &Harness,
    session: &agent::SessionId,
    tree: &agent::SessionTree,
    role: agent::MessageRole,
) -> Vec<String> {
    let messages: Vec<agent::MessageId> = tree
        .nodes
        .iter()
        .filter(|node| node.role == role)
        .map(|node| node.id)
        .collect();
    harness
        .bodies(session, &messages)
        .await
        .into_iter()
        .map(|body| body.text())
        .collect()
}

/// 第 `index` 次模型请求的全部消息正文，按请求内顺序。
fn request_contents(harness: &Harness, index: usize) -> Vec<String> {
    let requests = harness.provider.requests();
    let messages = requests[index]["messages"]
        .as_array()
        .expect("请求含 messages");
    messages
        .iter()
        .map(|message| message["content"].as_str().unwrap_or_default().to_owned())
        .collect()
}

/// 等待标记文件出现：`bash` 工具已经开始执行。
async fn wait_for_marker(harness: &Harness, name: &str) {
    let marker = harness.workspace_path("workspace").join(name);
    for _ in 0..250 {
        if marker.exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("工具未在预期时间内开始执行");
}

/// 留下标记后执行一秒的命令。工具不被打断时该命令能正常结束。
fn slow_command(marker: &str) -> String {
    format!(r#"{{"command":"touch {marker}; sleep 1"}}"#)
}

/// ADR 0039「插话在步骤边界接入当前 run」：插话在工具执行中提交，下一次模型请求带上它，
/// 同一个 run 继续，正在执行的工具不被打断。
#[tokio::test]
async fn steers_into_running_run_at_step_boundary() {
    let harness = Harness::start_with(
        agent::EngineConfig::default(),
        vec![
            Script::ToolCall {
                id: "call_slow".into(),
                name: "bash".into(),
                arguments: slow_command("steer-marker"),
            },
            Script::Text("按修正继续".into()),
        ],
    )
    .await;
    let session = harness.session("workspace").await;
    harness.submit_message(&session.id, "先跑一条命令").await;
    wait_for_marker(&harness, "steer-marker").await;

    let response = harness
        .submit_steer_message(&session.id, "改成只改领域层")
        .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let item: agent::QueueItem = response.json().await.expect("解析待插入项");
    assert!(item.pending_steer, "插话进入待插入状态");

    let pending = harness.tree(&session.id).await;
    assert!(pending.session.busy, "插话不结束当前 run");
    assert_eq!(queue_texts(&pending), vec!["改成只改领域层"]);
    assert_eq!(pending.session.queue_length, 1);
    assert_eq!(
        role_texts(&harness, &session.id, &pending, agent::MessageRole::User).await,
        vec!["先跑一条命令", "改成只改领域层"],
        "待插入消息在生效前就以用户节点出现"
    );

    let tree = harness.wait_idle(&session.id).await;
    assert_eq!(tree.runs.len(), 1, "插话复用同一个 run");
    assert_eq!(tree.runs[0].status, agent::RunStatus::Succeeded);
    assert!(tree.queue.is_empty(), "接入后队列项被消费");

    let tool = tree
        .nodes
        .iter()
        .find(|node| node.role == agent::MessageRole::Tool)
        .expect("工具节点已入树");
    let states: Vec<agent::ToolResultState> = harness
        .body(&session.id, tool.id)
        .await
        .tool_results()
        .map(|(_, state)| state.clone())
        .collect();
    assert!(
        matches!(
            states.first(),
            Some(agent::ToolResultState::Finished {
                outcome: agent::ToolOutcome::Succeeded { .. }
            })
        ),
        "插话不打断正在执行的工具: {states:?}"
    );

    let contents = request_contents(&harness, 1);
    assert_eq!(
        contents.last().map(String::as_str),
        Some("改成只改领域层"),
        "下一次模型请求带上插话"
    );

    let events = recorded_events(&harness.database).await;
    assert!(
        events.iter().any(|event| {
            matches!(event, agent::FactEvent::MessageQueued { item, .. } if item.pending_steer)
        }),
        "入队事件标记待插入"
    );
    let applied = events.iter().find_map(|event| match event {
        agent::FactEvent::SteerApplied {
            run_id,
            item_id,
            message_id,
            ..
        } => Some((*run_id, *item_id, *message_id)),
        _ => None,
    });
    let (run_id, item_id, message_id) = applied.expect("记录插话生效事件");
    assert_eq!(run_id, tree.runs[0].id);
    assert_eq!(item_id, item.id);
    assert_eq!(
        harness.text(&session.id, message_id).await,
        "改成只改领域层"
    );
}

/// ADR 0039：多条插话按提交顺序插入，且都进入同一次模型请求。
#[tokio::test]
async fn inserts_multiple_steers_in_submission_order() {
    let harness = Harness::start_with(
        agent::EngineConfig::default(),
        vec![
            Script::ToolCall {
                id: "call_slow".into(),
                name: "bash".into(),
                arguments: slow_command("steer-order-marker"),
            },
            Script::Text("按两条修正继续".into()),
        ],
    )
    .await;
    let session = harness.session("workspace").await;
    harness.submit_message(&session.id, "先跑一条命令").await;
    wait_for_marker(&harness, "steer-order-marker").await;

    for text in ["修正一", "修正二"] {
        let response = harness.submit_steer_message(&session.id, text).await;
        assert_eq!(response.status(), StatusCode::CREATED);
    }

    let tree = harness.wait_idle(&session.id).await;
    assert_eq!(tree.runs.len(), 1);
    assert!(tree.queue.is_empty());
    assert_eq!(
        role_texts(&harness, &session.id, &tree, agent::MessageRole::User).await,
        vec!["先跑一条命令", "修正一", "修正二"]
    );
    let contents = request_contents(&harness, 1);
    assert_eq!(
        contents
            .iter()
            .rev()
            .take(2)
            .cloned()
            .collect::<Vec<String>>(),
        vec!["修正二", "修正一"],
        "两条插话按提交顺序排在工具结果之后"
    );
}

/// 规格 user story A-15：显式 `mode=queue` 与缺省一致，等当前 run 结束才执行。
#[tokio::test]
async fn queues_messages_when_mode_is_queue() {
    let harness = Harness::start_with(agent::EngineConfig::default(), vec![Script::Hang]).await;
    let session = harness.session("workspace").await;
    harness.submit_message(&session.id, "第一条").await;
    harness.wait_busy(&session.id).await;

    let response = harness.submit_message(&session.id, "第二条").await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let item: agent::QueueItem = response.json().await.expect("解析队列项");
    assert!(!item.pending_steer, "排队语义不产生待插入项");

    let pending = harness.tree(&session.id).await;
    assert_eq!(queue_texts(&pending), vec!["第二条"]);
    assert_eq!(
        role_texts(&harness, &session.id, &pending, agent::MessageRole::User).await,
        vec!["第一条"],
        "排队消息此时还不是消息节点"
    );

    harness.cancel_run(&session.id).await;
    harness.wait_idle(&session.id).await;
}

/// ADR 0039：没有活动 run 时插话退化为普通提交，不重复创建用户节点。
#[tokio::test]
async fn treats_steer_without_running_run_as_queue() {
    let harness = Harness::start_with(
        agent::EngineConfig::default(),
        vec![Script::Text("好的".into())],
    )
    .await;
    let session = harness.session("workspace").await;

    let response = harness
        .submit_steer_message(&session.id, "没有 run 时插话")
        .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let item: agent::QueueItem = response.json().await.expect("解析队列项");
    assert!(!item.pending_steer, "没有活动 run 时插话退化为排队");

    let tree = harness.wait_idle(&session.id).await;
    assert_eq!(tree.runs.len(), 1);
    assert_eq!(
        role_texts(&harness, &session.id, &tree, agent::MessageRole::User).await,
        vec!["没有 run 时插话"],
        "用户节点只创建一次"
    );
    let contents = request_contents(&harness, 0);
    assert_eq!(contents.last().map(String::as_str), Some("没有 run 时插话"));
}

/// ADR 0039：取消后 run 以 `cancelled` 结束，未消费的插话留在树上并降级为排队，
/// 由下一次 run 执行。
#[tokio::test]
async fn downgrades_pending_steer_to_queue_on_cancel() {
    let harness = Harness::start_with(
        agent::EngineConfig::default(),
        vec![Script::Hang, Script::Text("取消后回答".into())],
    )
    .await;
    let session = harness.session("workspace").await;
    harness.submit_message(&session.id, "被打断的任务").await;
    harness.wait_busy(&session.id).await;
    harness.wait_for_requests(1).await;

    let response = harness
        .submit_steer_message(&session.id, "改为另一个方向")
        .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let item: agent::QueueItem = response.json().await.expect("解析待插入项");
    assert!(item.pending_steer);

    harness.cancel_run(&session.id).await;

    let tree = harness.wait_idle(&session.id).await;
    assert_eq!(tree.runs.len(), 2, "降级后的插话由下一次 run 执行");
    assert_eq!(tree.runs[0].status, agent::RunStatus::Cancelled);
    assert_eq!(tree.runs[1].status, agent::RunStatus::Succeeded);
    assert_eq!(
        role_texts(&harness, &session.id, &tree, agent::MessageRole::User).await,
        vec!["被打断的任务", "改为另一个方向"],
        "插话消息留在树上"
    );
    assert_eq!(
        role_texts(&harness, &session.id, &tree, agent::MessageRole::Assistant).await,
        vec!["取消后回答"]
    );

    let events = recorded_events(&harness.database).await;
    let downgraded = events.iter().find_map(|event| match event {
        agent::FactEvent::SteerDowngraded { item, .. } => Some(item.clone()),
        _ => None,
    });
    let downgraded = downgraded.expect("记录插话降级事件");
    assert_eq!(downgraded.id, item.id);
    assert!(!downgraded.pending_steer, "降级后不再是待插入项");
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, agent::FactEvent::SteerApplied { .. })),
        "被取消的 run 没有接入过插话"
    );
}

/// ADR 0039：降级后的插话排在更早的队列项之后。锚点已随链送达时从链尾接续，
/// 不把 active leaf 拽回，也不丢消息。
#[tokio::test]
async fn keeps_transcript_after_anchor_delivered_by_later_run() {
    let harness = Harness::start_with(
        agent::EngineConfig::default(),
        vec![
            Script::Hang,
            Script::Text("第二条完成".into()),
            Script::Text("第三条完成".into()),
        ],
    )
    .await;
    let session = harness.session("workspace").await;
    harness.submit_message(&session.id, "第一条").await;
    harness.wait_busy(&session.id).await;
    harness.wait_for_requests(1).await;
    harness.submit_message(&session.id, "第二条").await;
    harness.submit_message(&session.id, "第三条").await;
    let steer: agent::QueueItem = harness
        .submit_steer_message(&session.id, "第四条")
        .await
        .json()
        .await
        .expect("解析待插入项");
    assert!(steer.pending_steer);

    harness.cancel_run(&session.id).await;
    let tree = harness.wait_idle(&session.id).await;

    assert_eq!(
        role_texts(&harness, &session.id, &tree, agent::MessageRole::User).await,
        vec!["第一条", "第四条", "第二条", "第三条"],
        "降级后的插话在链上，后续队列项接在它后面"
    );
    assert!(
        request_contents(&harness, 1)
            .iter()
            .any(|content| content == "第四条"),
        "降级插话随第二条的请求送达模型"
    );
    assert_eq!(
        role_texts(&harness, &session.id, &tree, agent::MessageRole::Assistant).await,
        vec!["第二条完成", "第三条完成"],
        "锚点已送达，不重新产生回答"
    );
    assert!(tree.queue.is_empty());
}

/// ADR 0039：提交消息必须写明 `mode`，省略即协议错误，不再退化为排队。
#[tokio::test]
async fn rejects_submission_without_mode() {
    let harness = Harness::start().await;
    let session = harness.session("workspace").await;

    let response = harness
        .client
        .post(harness.url(&format!("/api/sessions/{}/messages", session.id.as_str())))
        .json(&serde_json::json!({ "text": "没有 mode" }))
        .send()
        .await
        .expect("提交消息");
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(error_code(response).await, agent::ErrorCode::InvalidRequest);

    let tree = harness.tree(&session.id).await;
    assert!(tree.queue.is_empty(), "被拒绝的请求不入队");
    assert!(tree.nodes.is_empty());
}
