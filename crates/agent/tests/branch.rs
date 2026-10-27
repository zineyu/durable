use reqwest::StatusCode;

mod support;

use support::provider::ToolCallSpec;
use support::{Harness, Script, error_code, recorded_facts};

/// 消息树中 root 到 active leaf 的节点 id。走与生产相同的树接口。
fn transcript_ids(tree: &agent::SessionTreeSnapshot) -> Vec<agent::MessageId> {
    let Some(leaf) = tree.active_leaf else {
        return Vec::new();
    };
    agent::MessageTree::new(tree.nodes.clone())
        .transcript(leaf)
        .into_iter()
        .map(|node| node.id)
        .collect()
}

/// 规格 user story A-16：编辑用户消息产生同父兄弟节点，active leaf 移到新分支并执行。
#[tokio::test]
async fn edits_user_message_into_sibling_branch() {
    let harness = Harness::start_with(
        agent::EngineConfig::default(),
        vec![Script::Text("A".into()), Script::Text("B".into())],
    )
    .await;
    let session = harness.session("workspace").await;
    harness.submit_message(&session.id, "原问题").await;
    let tree = harness.wait_idle(&session.id).await;
    let original_user = tree.nodes[0].id;
    let original_assistant = tree.nodes[1].id;

    let response = harness
        .edit_message(&session.id, original_user, "改写问题")
        .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let edited = response
        .json::<agent::MessageSkeleton>()
        .await
        .expect("解析新用户节点");
    assert_eq!(edited.role, agent::MessageRole::User);
    assert_eq!(edited.parent_id, None, "与目标用户消息同父");
    assert_ne!(edited.id, original_user);

    let tree = harness.wait_idle(&session.id).await;
    assert_eq!(
        tree.nodes.len(),
        4,
        "旧分支保留，新分支追加用户与 assistant 节点"
    );
    assert_eq!(tree.runs.len(), 2);

    let original = tree
        .nodes
        .iter()
        .find(|node| node.id == original_user)
        .expect("旧用户节点保留");
    assert_eq!(
        harness.text(&session.id, original.id).await,
        "原问题",
        "旧用户节点正文保留"
    );
    assert_eq!(
        original_assistant, tree.nodes[1].id,
        "旧 assistant 节点保留"
    );

    let new_user = tree
        .nodes
        .iter()
        .find(|node| node.id == edited.id)
        .expect("新用户节点");
    assert_eq!(new_user.parent_id, None);
    assert_eq!(
        harness.text(&session.id, new_user.id).await,
        "改写问题",
        "新用户节点正文是改写后的文本"
    );
    assert_eq!(
        new_user.run_id,
        Some(tree.runs[1].id),
        "新用户节点绑定新 run"
    );

    let new_assistant = tree
        .nodes
        .iter()
        .find(|node| node.parent_id == Some(new_user.id))
        .expect("新 assistant 节点");
    assert_eq!(new_assistant.role, agent::MessageRole::Assistant);
    assert_eq!(
        harness.text(&session.id, new_assistant.id).await,
        "B",
        "新 assistant 节点正文来自第二次回复"
    );
    assert_eq!(new_assistant.run_id, Some(tree.runs[1].id));
    assert_eq!(tree.active_leaf, Some(new_assistant.id));

    assert_eq!(
        transcript_ids(&tree),
        vec![new_user.id, new_assistant.id],
        "transcript 是新分支的路径"
    );
    assert_eq!(tree.runs[0].status, agent::RunStatus::Succeeded);
}

/// 规格 user story A-17：重试 assistant 在相同父节点下新建 run，不重复用户消息。
#[tokio::test]
async fn retries_assistant_with_same_parent_without_new_user() {
    let harness = Harness::start_with(
        agent::EngineConfig::default(),
        vec![Script::Text("A".into()), Script::Text("B".into())],
    )
    .await;
    let session = harness.session("workspace").await;
    harness.submit_message(&session.id, "问题").await;
    let tree = harness.wait_idle(&session.id).await;
    let user = tree.nodes[0].id;
    let first_assistant = tree.nodes[1].id;

    let response = harness.retry_message(&session.id, first_assistant).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let tree = harness.wait_idle(&session.id).await;
    assert_eq!(tree.nodes.len(), 3, "不重复创建用户消息");
    let users: Vec<&agent::MessageSkeleton> = tree
        .nodes
        .iter()
        .filter(|node| node.role == agent::MessageRole::User)
        .collect();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].id, user);
    assert_eq!(
        users[0].run_id,
        Some(tree.runs[0].id),
        "原用户节点的 run 关联不被覆盖"
    );

    let second_assistant = tree
        .nodes
        .iter()
        .find(|node| node.role == agent::MessageRole::Assistant && node.id != first_assistant)
        .expect("新 assistant 节点");
    assert_eq!(
        second_assistant.parent_id,
        Some(user),
        "与旧 assistant 同父"
    );
    assert_eq!(
        harness.text(&session.id, second_assistant.id).await,
        "B",
        "重试的 assistant 节点正文来自第二次回复"
    );
    assert_eq!(second_assistant.run_id, Some(tree.runs[1].id));
    assert_eq!(tree.active_leaf, Some(second_assistant.id));
    assert_eq!(transcript_ids(&tree), vec![user, second_assistant.id]);
    assert_eq!(tree.runs.len(), 2);
}

/// 规格 A-17：assistant 的父节点是 tool 节点时仍可重试，锚定在工具结果之后。
#[tokio::test]
async fn retries_assistant_whose_parent_is_a_tool_node() {
    let harness = Harness::start_with(
        agent::EngineConfig::default(),
        vec![
            Script::ToolCalls(vec![ToolCallSpec::new(
                "call_1",
                "bash",
                r#"{"command":"true"}"#,
            )]),
            Script::Text("工具之后".into()),
            Script::Text("重试的回答".into()),
        ],
    )
    .await;
    let session = harness.session("workspace").await;
    harness.submit_message(&session.id, "执行工具").await;
    let tree = harness.wait_idle(&session.id).await;

    let first = tree
        .nodes
        .iter()
        .rev()
        .find(|node| node.role == agent::MessageRole::Assistant)
        .expect("assistant 节点")
        .clone();
    let parent = tree
        .nodes
        .iter()
        .find(|node| Some(node.id) == first.parent_id)
        .expect("父节点");
    assert_eq!(parent.role, agent::MessageRole::Tool, "父节点是工具结果");

    let response = harness.retry_message(&session.id, first.id).await;
    assert_eq!(
        response.status(),
        StatusCode::NO_CONTENT,
        "重试以工具节点为锚，不读取它的正文"
    );

    let tree = harness.wait_idle(&session.id).await;
    let retried = tree
        .nodes
        .iter()
        .rev()
        .find(|node| node.role == agent::MessageRole::Assistant && node.id != first.id)
        .expect("新的 assistant 节点");
    assert_eq!(retried.parent_id, Some(parent.id), "与旧 assistant 同父");
    assert_eq!(
        harness.text(&session.id, retried.id).await,
        "重试的回答",
        "重试的正文来自第三次回复"
    );
}

/// 规格 user story A-18：切换 active leaf 移到目标子树中最近创建的叶子。
#[tokio::test]
async fn switches_active_leaf_to_subtree_leaf() {
    let harness = Harness::start_with(
        agent::EngineConfig::default(),
        vec![
            Script::Text("A".into()),
            Script::Text("B".into()),
            Script::Text("C".into()),
        ],
    )
    .await;
    let session = harness.session("workspace").await;
    harness.submit_message(&session.id, "原问题").await;
    let tree = harness.wait_idle(&session.id).await;
    let original_user = tree.nodes[0].id;
    let original_assistant = tree.nodes[1].id;

    harness
        .edit_message(&session.id, original_user, "改写问题")
        .await;
    let tree = harness.wait_idle(&session.id).await;
    let new_user = tree.nodes[2].id;
    let new_assistant = tree.nodes[3].id;

    let response = harness.switch_active_leaf(&session.id, original_user).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let tree = harness.tree(&session.id).await;
    assert_eq!(tree.active_leaf, Some(original_assistant));
    assert_eq!(
        transcript_ids(&tree),
        vec![original_user, original_assistant]
    );

    let response = harness.switch_active_leaf(&session.id, new_user).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let tree = harness.tree(&session.id).await;
    assert_eq!(tree.active_leaf, Some(new_assistant));
    assert_eq!(transcript_ids(&tree), vec![new_user, new_assistant]);
}

/// 规格「事实事件」：分支切换写入 `active_leaf_changed` 并持久化。
#[tokio::test]
async fn records_active_leaf_change_and_survives_restart() {
    let harness = Harness::start_with(
        agent::EngineConfig::default(),
        vec![Script::Text("A".into()), Script::Text("B".into())],
    )
    .await;
    let session = harness.session("workspace").await;
    harness.submit_message(&session.id, "原问题").await;
    let tree = harness.wait_idle(&session.id).await;
    let original_user = tree.nodes[0].id;
    let original_assistant = tree.nodes[1].id;
    harness
        .edit_message(&session.id, original_user, "改写问题")
        .await;
    harness.wait_idle(&session.id).await;

    harness.switch_active_leaf(&session.id, original_user).await;

    let facts = recorded_facts(&harness.database).await;
    let fact = facts
        .iter()
        .rev()
        .find(|fact| matches!(fact.event, agent::FactEvent::ActiveLeafChanged { .. }))
        .expect("记录了 active leaf 变更");
    let agent::FactEvent::ActiveLeafChanged { active_leaf, .. } = &fact.event else {
        panic!("事件是 active_leaf_changed");
    };
    let recorded = fact
        .session
        .as_ref()
        .expect("active leaf 变更带 session 视图");
    assert_eq!(*active_leaf, Some(original_assistant));
    assert_eq!(recorded.id, session.id);

    let bootstrap = harness.bootstrap().await;
    assert!(bootstrap.max_event_id > 0);

    let harness = harness.restart().await;
    let tree = harness.tree(&session.id).await;
    assert_eq!(
        tree.active_leaf,
        Some(original_assistant),
        "重启后 active leaf 恢复"
    );
    assert_eq!(
        transcript_ids(&tree),
        vec![original_user, original_assistant]
    );
}

/// 规格 user story A-19：run 进行中拒绝编辑、重试与切换，返回 `run_in_progress`。
#[tokio::test]
async fn rejects_branch_commands_while_running() {
    let harness = Harness::start_with(
        agent::EngineConfig::default(),
        vec![Script::Text("A".into()), Script::Hang],
    )
    .await;
    let session = harness.session("workspace").await;
    harness.submit_message(&session.id, "先完成一轮").await;
    let tree = harness.wait_idle(&session.id).await;
    let user = tree.nodes[0].id;
    let assistant = tree.nodes[1].id;

    harness.submit_message(&session.id, "运行中").await;
    harness.wait_busy(&session.id).await;
    harness.wait_for_requests(2).await;

    for response in [
        harness.edit_message(&session.id, user, "改写").await,
        harness.retry_message(&session.id, assistant).await,
        harness.switch_active_leaf(&session.id, user).await,
    ] {
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert_eq!(error_code(response).await, agent::ErrorCode::RunInProgress);
    }

    harness.cancel_run(&session.id).await;
    harness.wait_idle(&session.id).await;
}

/// 规格 user story A-19：队列非空时拒绝分支命令，返回 `queue_not_empty`。
#[tokio::test]
async fn rejects_branch_commands_with_pending_queue() {
    let harness = Harness::start_with(
        agent::EngineConfig::default(),
        vec![
            Script::Text("A".into()),
            Script::Hang,
            Script::Text("完成".into()),
        ],
    )
    .await;
    let session = harness.session("workspace").await;
    harness.submit_message(&session.id, "先完成一轮").await;
    let tree = harness.wait_idle(&session.id).await;
    let user = tree.nodes[0].id;
    let assistant = tree.nodes[1].id;

    harness.submit_message(&session.id, "运行中").await;
    harness.submit_message(&session.id, "排队中").await;
    harness
        .wait_for_tree(&session.id, |tree| !tree.queue.is_empty())
        .await;
    harness.wait_for_requests(2).await;

    for response in [
        harness.edit_message(&session.id, user, "改写").await,
        harness.retry_message(&session.id, assistant).await,
        harness.switch_active_leaf(&session.id, user).await,
    ] {
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert_eq!(error_code(response).await, agent::ErrorCode::QueueNotEmpty);
    }

    harness.cancel_run(&session.id).await;
    harness.wait_idle(&session.id).await;
}

/// 规格「边界解析」：目标消息不存在或角色不匹配时返回显式错误。
#[tokio::test]
async fn rejects_missing_or_mismatched_targets() {
    let harness = Harness::start_with(
        agent::EngineConfig::default(),
        vec![Script::Text("A".into())],
    )
    .await;
    let session = harness.session("workspace").await;
    harness.submit_message(&session.id, "问题").await;
    let tree = harness.wait_idle(&session.id).await;
    let user = tree.nodes[0].id;
    let assistant = tree.nodes[1].id;
    let missing = agent::MessageId::parse(9999).expect("合法消息 id");

    let response = harness.edit_message(&session.id, assistant, "改写").await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        error_code(response).await,
        agent::ErrorCode::MessageRoleMismatch
    );

    let response = harness.retry_message(&session.id, user).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        error_code(response).await,
        agent::ErrorCode::MessageRoleMismatch
    );

    let response = harness.switch_active_leaf(&session.id, missing).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        error_code(response).await,
        agent::ErrorCode::MessageNotFound
    );

    let response = harness.edit_message(&session.id, user, "   ").await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(error_code(response).await, agent::ErrorCode::InvalidMessage);
}
