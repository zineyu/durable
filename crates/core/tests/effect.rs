//! 副作用类别表与证据落点：六类各表态一次，证据经唯一入口写进现有领域行。
//! 类别与策略在 `crates/effects`，落点实现在 `crate::effect`，写入线在 `crate::store`。

#![expect(
    clippy::expect_used,
    reason = "内核测试用 expect 直接报告夹具与断言失败。"
)]

mod support;

use std::sync::Arc;

use durable::{
    ContentPart, EffectClass, EffectIntent, EffectObservation, MessageId, RunId, SessionId, Store,
    StoreError, ToolOutcome, ToolResultState,
};
use effects::policy::{Effect, EffectScope, Evidence, Replay};
use effects::token::{Abandon, Lost, Outcome, Transition, abandon, declare, dispatch, perform};
use support::{enqueue, max_event_id, store_with_session, tree};
use tempfile::TempDir;

/// 一个有 running run 的 session：队列里的一条消息已被取走，起点是用户节点。
async fn fixture() -> (TempDir, Store, SessionId, RunId) {
    let (directory, store, session) = store_with_session().await;
    enqueue(&store, &session, "问题").await;
    let start = store
        .start_run(&session)
        .await
        .expect("启动 run")
        .expect("队列非空时启动 run");
    (directory, store, session, start.run.id)
}

/// 读回 tool 节点的工具结果状态。
async fn tool_state(store: &Store, session: &SessionId, id: MessageId) -> ToolResultState {
    let bodies = store
        .message_bodies(session, &[id])
        .await
        .expect("读取块正文");
    let body = bodies
        .into_iter()
        .find(|body| body.id == id)
        .expect("节点在树上");
    body.parts
        .iter()
        .find_map(|part| match part {
            ContentPart::ToolResult { state, .. } => Some(state.clone()),
            _ => None,
        })
        .expect("工具节点带一个工具结果块")
}

fn tool_intent(
    class: EffectClass,
    session: &SessionId,
    run: RunId,
    message: MessageId,
) -> EffectIntent {
    EffectIntent::tool(class, session.clone(), run, message, "call_1")
}

fn observed_tool(state: ToolResultState) -> Outcome<EffectObservation> {
    Outcome::Seen(EffectObservation::Tool(state))
}

/// 一次工具类走完四个证据强度，每一步之后库里都能读回对应的状态。
#[tokio::test]
async fn a_tool_call_shows_the_declared_dispatched_and_finished_states() {
    let (_directory, store, session, run) = fixture().await;
    let message = store.mint_message_id().await.expect("铸造消息 id");

    let declared = declare(
        &store,
        tool_intent(EffectClass::Bash, &session, run, message),
    )
    .await
    .expect("登记写入成功");
    assert_eq!(
        tool_state(&store, &session, message).await,
        ToolResultState::Declared,
        "登记之后库里是已登记"
    );

    let dispatched = dispatch(&store, declared).await.expect("发货写入成功");
    assert_eq!(
        tool_state(&store, &session, message).await,
        ToolResultState::Dispatched,
        "发货之后库里是已下发"
    );

    let outcome = perform(&store, dispatched, |_token| async {
        observed_tool(ToolResultState::Finished {
            outcome: ToolOutcome::Succeeded {
                content: "完成".to_owned(),
            },
            duration_ms: 12,
        })
    })
    .await
    .expect("结局写入成功");

    assert!(matches!(outcome, Outcome::Seen(_)), "收货拿到观测");
    assert!(
        matches!(
            tool_state(&store, &session, message).await,
            ToolResultState::Finished { .. }
        ),
        "收货之后库里是有结局"
    );
}

/// 工作体报告丢失观测时原样落库；未下发就放弃同样落成「结果未知」。
#[tokio::test]
async fn lost_observations_land_as_unknown() {
    let (_directory, store, session, run) = fixture().await;

    let gone = store.mint_message_id().await.expect("铸造消息 id");
    let dispatched = dispatch(
        &store,
        declare(
            &store,
            tool_intent(EffectClass::EditFile, &session, run, gone),
        )
        .await
        .expect("登记写入成功"),
    )
    .await
    .expect("发货写入成功");
    let outcome = perform(&store, dispatched, |_token| async {
        Outcome::Lost(Lost::HostGone("链接断开".to_owned()))
    })
    .await
    .expect("断链结论写入成功");

    assert_eq!(
        outcome,
        Outcome::Lost(Lost::HostGone("链接断开".to_owned()))
    );
    assert_eq!(
        tool_state(&store, &session, gone).await,
        ToolResultState::Unknown
    );

    let cancelled = store.mint_message_id().await.expect("铸造消息 id");
    let abandoned = abandon(
        &store,
        declare(
            &store,
            tool_intent(EffectClass::ReadFile, &session, run, cancelled),
        )
        .await
        .expect("登记写入成功"),
        Abandon::Cancelled,
    )
    .await
    .expect("放弃写入成功");

    assert_eq!(abandoned, Outcome::Lost(Lost::Cancelled));
    assert_eq!(
        tool_state(&store, &session, cancelled).await,
        ToolResultState::Unknown
    );
}

/// 模型步骤与压缩的登记、下发与丢失观测都不写行；观测才是消息入树的那一刻。
#[tokio::test]
async fn model_steps_and_compactions_write_no_row_before_the_observation() {
    let (_directory, store, session, run) = fixture().await;
    let before = max_event_id(&store).await;
    let nodes_before = tree(&store, &session).await.nodes.len();

    let message = store.mint_message_id().await.expect("铸造消息 id");
    let dispatched = dispatch(
        &store,
        declare(
            &store,
            EffectIntent::model_step(session.clone(), run, message),
        )
        .await
        .expect("登记写入成功"),
    )
    .await
    .expect("发货写入成功");

    assert_eq!(max_event_id(&store).await, before, "登记与下发不写事实事件");
    assert_eq!(
        tree(&store, &session).await.nodes.len(),
        nodes_before,
        "登记与下发不写消息行"
    );

    store
        .write_progress(&session, run, "正在写")
        .await
        .expect("写入进度");
    let outcome = perform(&store, dispatched, |_token| async {
        Outcome::Seen(EffectObservation::Assistant {
            parts: vec![ContentPart::text("第一步")],
        })
    })
    .await
    .expect("观测写入成功");

    assert!(matches!(outcome, Outcome::Seen(_)), "步子定型");
    let snapshot = tree(&store, &session).await;
    assert!(
        snapshot.nodes.iter().any(|node| node.id == message),
        "assistant 消息入树"
    );
    assert!(
        store
            .running_run(&session)
            .await
            .expect("读取执行中的 run")
            .expect("run 仍在执行")
            .progress
            .is_none(),
        "定型与清空进度同一事务"
    );

    let compaction = store.mint_message_id().await.expect("铸造消息 id");
    let dispatched = dispatch(
        &store,
        declare(
            &store,
            EffectIntent::compaction(session.clone(), compaction),
        )
        .await
        .expect("登记写入成功"),
    )
    .await
    .expect("发货写入成功");
    let seen = perform(&store, dispatched, |_token| async {
        Outcome::Seen(EffectObservation::Compaction {
            parts: vec![ContentPart::text("摘要")],
        })
    })
    .await
    .expect("压缩观测写入成功");
    assert!(matches!(seen, Outcome::Seen(_)), "压缩拿到摘要");

    assert!(
        tree(&store, &session)
            .await
            .nodes
            .iter()
            .any(|node| node.id == compaction),
        "压缩消息入树"
    );

    // 丢失观测同样不写行：超时的证据就是「已下发、没观测」（ADR 0059）。
    timed_out_model_step_writes_no_row(&store, &session, run).await;
}

/// 模型步骤报告超时时，账本只收到一条丢失观测，库里什么都没有。
async fn timed_out_model_step_writes_no_row(store: &Store, session: &SessionId, run: RunId) {
    let before = max_event_id(store).await;
    let nodes_before = tree(store, session).await.nodes.len();
    let message = store.mint_message_id().await.expect("铸造消息 id");
    let dispatched = dispatch(
        store,
        declare(
            store,
            EffectIntent::model_step(session.clone(), run, message),
        )
        .await
        .expect("登记写入成功"),
    )
    .await
    .expect("发货写入成功");
    let outcome = perform(store, dispatched, |_token| async {
        Outcome::Lost(Lost::Timeout)
    })
    .await
    .expect("超时结论写入成功");
    assert_eq!(outcome, Outcome::Lost(Lost::Timeout), "超时原样返回");
    assert_eq!(max_event_id(store).await, before, "丢失观测不写事实事件");
    assert_eq!(
        tree(store, session).await.nodes.len(),
        nodes_before,
        "丢失观测不写消息行"
    );
}

/// 未登记的工具名没有类别：落点仍是 tool 节点，但不计入覆盖率。
#[tokio::test]
async fn an_unregistered_tool_lands_on_its_node_without_coverage() {
    let (_directory, store, session, run) = fixture().await;
    let message = store.mint_message_id().await.expect("铸造消息 id");
    let intent = || EffectIntent::unregistered_tool(session.clone(), run, message, "call_1");

    let declared = declare(&store, intent()).await.expect("登记写入成功");
    assert_eq!(
        tool_state(&store, &session, message).await,
        ToolResultState::Declared,
        "未登记的工具名照常入树"
    );

    let dispatched = dispatch(&store, declared).await.expect("发货写入成功");
    assert_eq!(
        tool_state(&store, &session, message).await,
        ToolResultState::Dispatched,
        "照常落成已下发"
    );

    let seen = perform(&store, dispatched, |_token| async {
        observed_tool(ToolResultState::Finished {
            outcome: ToolOutcome::Failed {
                code: durable::ToolErrorCode::UnknownTool,
                message: "未知工具: list_dir".to_owned(),
            },
            duration_ms: 3,
        })
    })
    .await
    .expect("观测写入成功");

    assert!(matches!(seen, Outcome::Seen(_)), "结局写入成功");
    assert!(
        store.effects_declared().is_empty(),
        "没有类别，因此不计入覆盖率"
    );
}

/// 落点与观测错配被拒绝，且不落任何行。
#[tokio::test]
async fn mismatched_landings_are_invariant_errors() {
    let (_directory, store, session, run) = fixture().await;
    let message = store.mint_message_id().await.expect("铸造消息 id");
    let dispatched = dispatch(
        &store,
        declare(
            &store,
            tool_intent(EffectClass::Bash, &session, run, message),
        )
        .await
        .expect("登记写入成功"),
    )
    .await
    .expect("发货写入成功");
    let before = max_event_id(&store).await;

    let error = perform(&store, dispatched, |_token| async {
        Outcome::Seen(EffectObservation::Assistant {
            parts: vec![ContentPart::text("不该落在这里")],
        })
    })
    .await
    .expect_err("工具落点不接受 assistant 观测");

    match error {
        StoreError::Invariant(message) => {
            assert!(message.contains("bash"), "文案带类别名：{message}");
            assert!(message.contains("assistant"), "文案带观测名：{message}");
        }
        other => panic!("应当是落点不变量错误，实际是 {other:?}"),
    }
    assert_eq!(max_event_id(&store).await, before, "拒绝时不写事实事件");
    assert_eq!(
        tool_state(&store, &session, message).await,
        ToolResultState::Dispatched,
        "拒绝时证据停在已下发"
    );

    let Err(error) = declare(
        &store,
        tool_intent(EffectClass::ModelStep, &session, run, message),
    )
    .await
    else {
        panic!("模型步骤不能落在 tool 节点上");
    };

    match error {
        StoreError::Invariant(message) => assert!(message.contains("model_step"), "{message}"),
        other => panic!("应当是落点不变量错误，实际是 {other:?}"),
    }
}

/// 覆盖率：记录发生过的类别，并报出声明覆盖却没出现的类别名。
#[tokio::test]
async fn coverage_records_each_declared_class_once() {
    let (_directory, store, session, run) = fixture().await;
    assert!(store.effects_declared().is_empty(), "还没有任何记录");
    assert_eq!(
        store.effect_gaps(),
        vec![
            "model_step",
            "compaction",
            "read_file",
            "write_file",
            "edit_file",
            "bash"
        ],
        "六类都声明覆盖，因此首次读出的都是缺口"
    );

    for class in [
        EffectClass::ReadFile,
        EffectClass::WriteFile,
        EffectClass::EditFile,
        EffectClass::Bash,
    ] {
        let message = store.mint_message_id().await.expect("铸造消息 id");
        let _declared = declare(&store, tool_intent(class, &session, run, message))
            .await
            .expect("登记写入成功");
    }

    assert_eq!(
        store.effects_declared(),
        vec![
            EffectClass::ReadFile,
            EffectClass::WriteFile,
            EffectClass::EditFile,
            EffectClass::Bash,
        ],
        "按首次发生的顺序，每个一次"
    );
    assert_eq!(
        store.effect_gaps(),
        vec!["model_step", "compaction"],
        "没出现过的类别被报出来"
    );

    let _model_step = declare(
        &store,
        EffectIntent::model_step(
            session.clone(),
            run,
            store.mint_message_id().await.expect("铸造"),
        ),
    )
    .await
    .expect("登记写入成功");
    let _compaction = declare(
        &store,
        EffectIntent::compaction(
            session.clone(),
            store.mint_message_id().await.expect("铸造"),
        ),
    )
    .await
    .expect("登记写入成功");
    assert!(store.effect_gaps().is_empty(), "六类都出现过");
}

/// 并发的证据写入互不插入：每条登记各提交一次入树与一次 active leaf 变更。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_declarations_keep_every_node_once() {
    const DECLARATIONS: usize = 4;
    let (_directory, store, session, run) = fixture().await;
    let before = max_event_id(&store).await;
    let store = Arc::new(store);

    let mut tasks = Vec::new();
    for _ in 0..DECLARATIONS {
        let store = Arc::clone(&store);
        let session = session.clone();
        tasks.push(tokio::spawn(async move {
            let message = store.mint_message_id().await.expect("铸造消息 id");
            let _declared = declare(
                &*store,
                tool_intent(EffectClass::ReadFile, &session, run, message),
            )
            .await
            .expect("登记写入成功");
        }));
    }
    for task in tasks {
        task.await.expect("登记任务完成");
    }

    let tools = tree(&store, &session)
        .await
        .nodes
        .into_iter()
        .filter(|node| durable::MessageRole::Tool == node.role)
        .count();
    assert_eq!(tools, DECLARATIONS, "每次登记各入树一个 tool 节点");
    assert_eq!(
        store.events_after(before).await.expect("回放事件").len(),
        DECLARATIONS * 2,
        "每次登记提交入树与 active leaf 两条事实事件"
    );
}

/// 标签是日志、告警与界面据以开口的字面量，因此改标签必须是一次有意的改动。
/// 清单本身由 `effects::effect_table!` 从同一份声明生成，不再需要手写副本。
#[test]
fn the_class_labels_are_stable_literals() {
    let names = EffectClass::ALL
        .iter()
        .map(|class| class.name())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        vec![
            "model_step",
            "compaction",
            "read_file",
            "write_file",
            "edit_file",
            "bash"
        ],
        "名字是日志与告警据以开口的字面量"
    );
}

/// 六类的表态：重放策略、工具归属与覆盖声明。
#[test]
fn the_table_states_replay_and_coverage_for_every_class() {
    let redo = [EffectClass::ModelStep, EffectClass::Compaction];
    let if_idempotent = [
        EffectClass::ReadFile,
        EffectClass::WriteFile,
        EffectClass::EditFile,
        EffectClass::Bash,
    ];

    for class in redo {
        assert_eq!(
            class.replay(Evidence::Dispatched),
            Replay::Redo,
            "{} 重发没有副作用",
            class.name()
        );
        assert!(!class.is_tool(), "{} 不落在 tool 节点上", class.name());
    }
    for class in if_idempotent {
        assert_eq!(
            class.replay(Evidence::Dispatched),
            Replay::IfIdempotent,
            "{} 按幂等标记分流",
            class.name()
        );
        assert!(class.is_tool(), "{} 落在 tool 节点上", class.name());
    }
    assert_eq!(
        EffectClass::of_tool("read_file"),
        Some(EffectClass::ReadFile),
        "工具名映射到类别"
    );
    assert_eq!(
        EffectClass::of_tool("write_file"),
        Some(EffectClass::WriteFile)
    );
    assert_eq!(
        EffectClass::of_tool("edit_file"),
        Some(EffectClass::EditFile)
    );
    assert_eq!(EffectClass::of_tool("bash"), Some(EffectClass::Bash));
    assert_eq!(
        EffectClass::of_tool("move_file"),
        None,
        "未登记的工具没有类别"
    );

    for class in EffectClass::ALL {
        assert_eq!(
            class.matrix_scope(),
            EffectScope::Covered,
            "{} 由崩溃矩阵覆盖",
            class.name()
        );
    }
}

/// 库里的证据写入都走写事务：一次 `Transition` 要么整条落库，要么什么都不留。
#[tokio::test]
async fn an_evidence_write_is_one_committed_transaction() {
    let (_directory, store, session, run) = fixture().await;
    let message = store.mint_message_id().await.expect("铸造消息 id");
    let before = max_event_id(&store).await;

    effects::token::Ledger::write(
        &store,
        Transition::Declared(tool_intent(EffectClass::WriteFile, &session, run, message)),
    )
    .await
    .expect("登记写入成功");

    let facts = store.events_after(before).await.expect("回放事件");
    assert_eq!(facts.len(), 2, "入树与 active leaf 在同一次提交里");
    assert_eq!(
        tool_state(&store, &session, message).await,
        ToolResultState::Declared
    );
}
