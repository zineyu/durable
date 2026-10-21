use std::time::Duration;

use reqwest::StatusCode;

mod support;

use support::{Harness, Script};

/// 规格「模型调用」：上下文超限以对应错误码结束 run，且不发出模型请求。
#[tokio::test]
async fn fails_run_when_context_limit_is_exceeded() {
    let config = agent::EngineConfig {
        context_char_limit: 20,
        ..agent::EngineConfig::default()
    };
    let harness = Harness::start_with(config, vec![Script::Text("不会用到".into())]).await;
    let session = harness.session("workspace").await;

    harness.submit_message(&session.id, "你好").await;
    let tree = harness.wait_idle(&session.id).await;

    let run = &tree.runs[0];
    assert_eq!(run.status, agent::RunStatus::Failed);
    let failure = run.failure.as_ref().expect("失败原因");
    assert_eq!(failure.code, agent::RunFailureCode::ContextLimitExceeded);
    assert!(harness.provider.requests().is_empty());
}

/// 规格「模型调用」：步数超限以对应错误码结束 run。
#[tokio::test]
async fn fails_run_when_step_limit_is_exceeded() {
    let config = agent::EngineConfig {
        max_steps: 2,
        ..agent::EngineConfig::default()
    };
    let harness = Harness::start_with(
        config,
        vec![
            Script::ToolCall {
                id: "call_1".into(),
                name: "bash".into(),
                arguments: "{}".into(),
            },
            Script::ToolCall {
                id: "call_2".into(),
                name: "bash".into(),
                arguments: "{}".into(),
            },
        ],
    )
    .await;
    let session = harness.session("workspace").await;

    harness.submit_message(&session.id, "重复调用").await;
    let tree = harness.wait_idle(&session.id).await;

    let run = &tree.runs[0];
    assert_eq!(run.status, agent::RunStatus::Failed);
    let failure = run.failure.as_ref().expect("失败原因");
    assert_eq!(failure.code, agent::RunFailureCode::StepLimitExceeded);
    assert_eq!(harness.provider.requests().len(), 2);
}

/// 规格「模型调用」：模型上游错误终止当前 run，队列继续。
#[tokio::test]
async fn fails_run_on_model_upstream_error_and_continues_queue() {
    let harness = Harness::start_with(
        agent::EngineConfig::default(),
        vec![
            Script::Fail {
                status: 500,
                retry_after: None,
            },
            Script::Text("恢复".into()),
        ],
    )
    .await;
    let session = harness.session("workspace").await;

    harness.submit_message(&session.id, "第一项").await;
    harness.submit_message(&session.id, "第二项").await;
    let tree = harness.wait_idle(&session.id).await;

    let failure = tree.runs[0].failure.as_ref().expect("失败原因");
    assert_eq!(failure.code, agent::RunFailureCode::ModelUpstream);
    assert_eq!(tree.runs[0].status, agent::RunStatus::Failed);
    assert_eq!(tree.runs[1].status, agent::RunStatus::Succeeded);
    assert_eq!(tree.nodes[3].text.as_str(), "恢复");
}

/// 规格「调度与会话状态」：重启把仍为 running 的 run 标记为 failed（进程中断）。
#[tokio::test]
async fn marks_interrupted_run_as_failed_after_restart() {
    let harness = Harness::start_with(agent::EngineConfig::default(), vec![Script::Hang]).await;
    let session = harness.session("workspace").await;
    harness.submit_message(&session.id, "长时间运行").await;
    harness.wait_busy(&session.id).await;

    let harness = harness.restart().await;
    let tree = harness.tree(&session.id).await;

    assert!(!tree.session.busy);
    let run = tree.runs.last().expect("有 run 记录");
    assert_eq!(run.status, agent::RunStatus::Failed);
    let failure = run.failure.as_ref().expect("失败原因");
    assert_eq!(failure.code, agent::RunFailureCode::Interrupted);
    assert_eq!(tree.queue.len(), 0, "队列在 run 开始时已消费");
}

/// 规格「协议」：单一全局 SSE 流同时输送 token 增量与事实事件。
#[tokio::test]
async fn streams_deltas_and_facts_over_sse() {
    let harness = Harness::start_with(
        agent::EngineConfig::default(),
        vec![Script::Text("流式输出".into())],
    )
    .await;
    let session = harness.session("workspace").await;
    let cursor = harness.bootstrap().await.max_event_id;
    let mut events = harness.subscribe(cursor).await;

    harness.submit_message(&session.id, "开始").await;

    let mut deltas = String::new();
    let mut kinds = Vec::new();
    loop {
        let event = tokio::time::timeout(Duration::from_secs(10), events.next())
            .await
            .expect("等待 SSE 事件")
            .expect("事件流未结束");
        match event {
            agent::StreamEvent::Delta { text, .. } => deltas.push_str(&text),
            agent::StreamEvent::Fact { event, id, .. } => {
                assert!(id > cursor, "实时事件 id 必须大于游标");
                let kind = serde_json::to_value(&event).expect("事件可序列化")["kind"]
                    .as_str()
                    .expect("事件有 kind")
                    .to_owned();
                kinds.push(kind);
                if matches!(event, agent::FactEvent::RunFinished { .. }) {
                    break;
                }
            }
            agent::StreamEvent::Retry { .. } => unreachable!("未限流时不应出现重试提示"),
            agent::StreamEvent::Resync { .. } => unreachable!("未落后时不应要求重连"),
        }
    }

    assert_eq!(deltas, "流式输出");
    assert_eq!(
        kinds,
        vec![
            "message_queued",
            "message_appended",
            "message_appended",
            "active_leaf_changed",
            "queue_item_removed",
            "run_started",
            "message_updated",
            "run_finished",
        ]
    );
}

/// ADR 0031：供应商限流时 run 以显式类别结束，说明带等待建议。
#[tokio::test]
async fn fails_run_as_rate_limited_when_retries_are_exhausted() {
    let harness = Harness::start_with(
        agent::EngineConfig {
            model_retry_limit: 2,
            model_retry_backoff: Duration::from_millis(10),
            ..agent::EngineConfig::default()
        },
        vec![
            Script::Fail {
                status: 429,
                retry_after: None,
            },
            Script::Fail {
                status: 429,
                retry_after: None,
            },
            Script::Fail {
                status: 429,
                retry_after: Some(2),
            },
            Script::Text("下一项".into()),
        ],
    )
    .await;
    let session = harness.session("workspace").await;

    harness.submit_message(&session.id, "第一项").await;
    harness.submit_message(&session.id, "第二项").await;
    let tree = harness.wait_idle(&session.id).await;

    let failure = tree.runs[0].failure.as_ref().expect("失败原因");
    assert_eq!(failure.code, agent::RunFailureCode::ModelRateLimited);
    assert!(
        failure.message.contains("429"),
        "说明要点明状态码: {}",
        failure.message
    );
    assert!(
        failure.message.contains("2000 毫秒"),
        "说明要带供应商建议的等待时长: {}",
        failure.message
    );
    assert_eq!(
        harness.provider.requests().len(),
        4,
        "第一次 run 用尽两次重试后不再请求，第二次 run 请求一次"
    );
    assert_eq!(tree.runs[1].status, agent::RunStatus::Succeeded, "队列继续");
    assert!(tree.queue.is_empty());
}

/// ADR 0031：瞬时限流自动重试，不写额外事实事件。
#[tokio::test]
async fn retries_rate_limited_call_and_reports_waiting_over_sse() {
    let harness = Harness::start_with(
        agent::EngineConfig {
            model_retry_backoff: Duration::from_millis(20),
            ..agent::EngineConfig::default()
        },
        vec![
            Script::Fail {
                status: 429,
                retry_after: None,
            },
            Script::Text("恢复".into()),
        ],
    )
    .await;
    let session = harness.session("workspace").await;
    let cursor = harness.bootstrap().await.max_event_id;
    let mut events = harness.subscribe(cursor).await;
    harness.submit_message(&session.id, "你好").await;

    let mut retry = None;
    let mut kinds = Vec::new();
    loop {
        let event = tokio::time::timeout(Duration::from_secs(10), events.next())
            .await
            .expect("等待 SSE 事件")
            .expect("事件流未结束");
        match event {
            agent::StreamEvent::Fact { event, .. } => {
                let kind = serde_json::to_value(&event).expect("事件可序列化")["kind"]
                    .as_str()
                    .expect("事件有 kind")
                    .to_owned();
                let finished = matches!(event, agent::FactEvent::RunFinished { .. });
                kinds.push(kind);
                if finished {
                    break;
                }
            }
            agent::StreamEvent::Retry {
                session_id,
                attempt,
                max_attempts,
                delay_ms,
            } => retry = Some((session_id, attempt, max_attempts, delay_ms)),
            agent::StreamEvent::Delta { .. } => {}
            agent::StreamEvent::Resync { .. } => unreachable!("未落后时不应要求重连"),
        }
    }

    let (retried_session, attempt, max_attempts, delay_ms) = retry.expect("限流必须广播重试提示");
    assert_eq!(retried_session, session.id);
    assert_eq!((attempt, max_attempts), (1, 3), "首次重试与默认上限");
    assert_eq!(delay_ms, 20, "提示里的等待时长就是本轮回退");
    assert_eq!(
        kinds,
        vec![
            "message_queued",
            "message_appended",
            "message_appended",
            "active_leaf_changed",
            "queue_item_removed",
            "run_started",
            "message_updated",
            "run_finished",
        ],
        "重试不产生额外事实事件"
    );

    let tree = harness.wait_idle(&session.id).await;
    assert_eq!(tree.runs[0].status, agent::RunStatus::Succeeded);
    assert_eq!(tree.nodes[1].text.as_str(), "恢复");
}

/// ADR 0031：`Retry-After` 优先于指数退避。
#[tokio::test]
async fn honors_retry_after_instead_of_backoff() {
    let harness = Harness::start_with(
        agent::EngineConfig {
            model_retry_backoff: Duration::from_secs(5),
            ..agent::EngineConfig::default()
        },
        vec![
            Script::Fail {
                status: 429,
                retry_after: Some(0),
            },
            Script::Text("立刻恢复".into()),
        ],
    )
    .await;
    let session = harness.session("workspace").await;
    let started = std::time::Instant::now();
    harness.submit_message(&session.id, "你好").await;
    let tree = harness.wait_idle(&session.id).await;

    assert_eq!(tree.runs[0].status, agent::RunStatus::Succeeded);
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "必须服从 Retry-After 而不是 5 秒退避，实际 {:?}",
        started.elapsed()
    );
}

/// 重试次数为 0 表示不重试：限流立刻以显式类别失败。
#[tokio::test]
async fn does_not_retry_when_limit_is_zero() {
    let harness = Harness::start_with(
        agent::EngineConfig {
            model_retry_limit: 0,
            ..agent::EngineConfig::default()
        },
        vec![
            Script::Fail {
                status: 429,
                retry_after: None,
            },
            Script::Text("不会用到".into()),
        ],
    )
    .await;
    let session = harness.session("workspace").await;
    harness.submit_message(&session.id, "你好").await;
    let tree = harness.wait_idle(&session.id).await;

    let failure = tree.runs[0].failure.as_ref().expect("失败原因");
    assert_eq!(failure.code, agent::RunFailureCode::ModelRateLimited);
    assert_eq!(harness.provider.requests().len(), 1);
}

/// 规格：取消优先于重试等待，等待中取消立刻结束 run。
#[tokio::test]
async fn cancels_run_during_retry_wait() {
    let harness = Harness::start_with(
        agent::EngineConfig {
            model_retry_backoff: Duration::from_secs(30),
            ..agent::EngineConfig::default()
        },
        vec![Script::Fail {
            status: 429,
            retry_after: None,
        }],
    )
    .await;
    let session = harness.session("workspace").await;
    let cursor = harness.bootstrap().await.max_event_id;
    let mut events = harness.subscribe(cursor).await;
    harness.submit_message(&session.id, "你好").await;

    loop {
        let event = tokio::time::timeout(Duration::from_secs(10), events.next())
            .await
            .expect("等待 SSE 事件")
            .expect("事件流未结束");
        if matches!(event, agent::StreamEvent::Retry { .. }) {
            break;
        }
    }

    let response = harness.cancel_run(&session.id).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let tree = harness.wait_idle(&session.id).await;

    assert_eq!(tree.runs[0].status, agent::RunStatus::Cancelled);
    assert_eq!(tree.runs[0].failure, None);
    assert_eq!(harness.provider.requests().len(), 1, "取消之后不再重试");
}
