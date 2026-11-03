//! 崩溃前缀矩阵：把「任意位置崩溃」降成「每个提交点重启一次」。
//!
//! 事务是原子的，所以一次崩溃只可能落在两次提交之间；`Store::open_recording` 在每个
//! 提交点留下一份副本，因此遍历副本就等于遍历全部崩溃点。每个副本上断言同一组性质：
//! 恢复收敛、不变量自洽、历史只增不减、没有执行中的工具、非幂等副作用至多一次。
//! 决策与局限见 ADR 0056。

mod support;

use durable::{RunStatus, ToolResultState};
use support::matrix::{Call, Step, restart, run_scenario, tree_has};

/// 一个场景：脚本、它声明的调用（带幂等标记）与副本数量下限。
struct Scenario {
    name: &'static str,
    steps: Vec<Step>,
    calls: Vec<(&'static str, bool)>,
}

fn scenarios() -> Vec<Scenario> {
    vec![
        Scenario {
            name: "纯文本回答",
            steps: vec![Step::Text("一句话回答".to_owned())],
            calls: Vec::new(),
        },
        Scenario {
            name: "一个工具批次",
            steps: vec![
                Step::Calls(vec![Call::new("call_bash", "bash")]),
                Step::Text("工具之后的回答".to_owned()),
            ],
            calls: vec![("call_bash", false)],
        },
        Scenario {
            name: "多步与幂等工具",
            steps: vec![
                Step::Calls(vec![
                    Call::new("call_read", "read_file"),
                    Call::new("call_bash", "bash"),
                ]),
                Step::Calls(vec![Call::new("call_read_again", "read_file")]),
                Step::Text("收尾".to_owned()),
            ],
            calls: vec![
                ("call_read", true),
                ("call_bash", false),
                ("call_read_again", true),
            ],
        },
    ]
}

/// 规格 user story A-24：重启前的 running run 从树尾续跑。ADR 0056：每个提交点都成立。
#[tokio::test]
async fn every_commit_prefix_recovers() {
    for scenario in scenarios() {
        let directory = tempfile::tempdir().expect("创建临时目录");
        let world = run_scenario(directory.path(), scenario.steps.clone()).await;
        assert!(
            world.prefixes.len() >= 3,
            "{}：提交点太少，矩阵没有意义",
            scenario.name
        );
        let mut mid_run = 0;
        for (prefix, path) in &world.prefixes {
            let Some(recovered) = restart(path, &world.session).await else {
                continue;
            };
            if recovered.was_busy {
                mid_run += 1;
            }
            check_prefix(scenario.name, *prefix, &world, &recovered, &scenario.calls);
        }
        assert!(
            mid_run >= 1,
            "{}：没有任何提交点落在 run 中间，矩阵没覆盖到续跑",
            scenario.name
        );
    }
}

fn check_prefix(
    scenario: &str,
    prefix: u64,
    world: &support::matrix::World,
    recovered: &support::matrix::Restart,
    calls: &[(&str, bool)],
) {
    assert!(
        !recovered.tree.session.busy && recovered.tree.queue.is_empty(),
        "{scenario} 的提交点 {prefix}：恢复后没有收敛"
    );
    assert!(
        recovered.invariants.is_empty(),
        "{scenario} 的提交点 {prefix}：不变量有 {} 条违例: {:?}",
        recovered.invariants.len(),
        recovered.invariants
    );
    for node in &recovered.tree.nodes {
        assert_ne!(
            node.run_id.map(|_| ()),
            None,
            "{scenario} 的提交点 {prefix}：节点既不挂在 run 上也不是压缩消息"
        );
    }
    for run in &recovered.tree.runs {
        assert_ne!(
            run.status,
            RunStatus::Running,
            "{scenario} 的提交点 {prefix}：恢复后仍有一条 running 的 run"
        );
    }
    for (id, _) in &recovered.before {
        assert!(
            tree_has(&recovered.tree, *id),
            "{scenario} 的提交点 {prefix}：崩溃前的消息 {id:?} 从链上消失了"
        );
    }
    for (call_id, state) in &recovered.after {
        assert!(
            !matches!(state, ToolResultState::Running),
            "{scenario} 的提交点 {prefix}：调用 {call_id} 停在执行中"
        );
    }
    for (call_id, idempotent) in calls {
        let before = world.effects_before(call_id, prefix);
        let reruns = recovered.reruns_of(call_id);
        let limit = if *idempotent { 2 } else { 1 };
        assert!(
            before + reruns <= limit,
            "{scenario} 的提交点 {prefix}：{call_id} 被执行了 {} 次（崩溃前 {before}、重启后 {reruns}），上限 {limit}",
            before + reruns
        );
        if reruns > 0 {
            assert!(
                matches!(
                    recovered.state_of(call_id),
                    Some(ToolResultState::Finished { .. })
                ),
                "{scenario} 的提交点 {prefix}：{call_id} 被重跑却没有留下结局"
            );
        }
    }
}

/// 已知缺陷 #67：工具节点已入树、帧尚未下发时，非幂等调用被写成「结果未知」而不重跑，
/// 这次调用的工作因此凭空消失。这条用例描述现状；#67 落地后 `unknown` 的计数应当变成 0，
/// 用例也要改成断言「未下发的调用被重跑」。
#[tokio::test]
async fn undispatched_calls_are_written_as_unknown_without_a_rerun() {
    let directory = tempfile::tempdir().expect("创建临时目录");
    let world = run_scenario(
        directory.path(),
        vec![
            Step::Calls(vec![Call::new("call_bash", "bash")]),
            Step::Text("工具之后的回答".to_owned()),
        ],
    )
    .await;

    let mut undispatched = 0;
    for (prefix, path) in &world.prefixes {
        let Some(recovered) = restart(path, &world.session).await else {
            continue;
        };
        let already_running =
            recovered
                .before
                .iter()
                .flat_map(|(_, states)| states)
                .any(|(call_id, state)| {
                    call_id == "call_bash" && matches!(state, ToolResultState::Running)
                });
        if !already_running {
            continue;
        }
        undispatched += 1;
        assert_eq!(
            recovered.reruns_of("call_bash"),
            0,
            "提交点 {prefix}：非幂等调用没有被重跑"
        );
        assert_eq!(
            recovered.state_of("call_bash"),
            Some(ToolResultState::Unknown),
            "提交点 {prefix}：非幂等调用的结局应当是「结果未知」"
        );
    }
    assert!(
        undispatched >= 1,
        "没有任何提交点停在「工具节点已入树、尚未下发」，用例没覆盖到这条分支"
    );
}
