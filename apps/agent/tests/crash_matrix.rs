//! 崩溃前缀矩阵：把「任意位置崩溃」降成「每个提交点重启一次」。
//!
//! 事务是原子的，因此一次崩溃只可能落在两次提交之间；`Store::open_recording` 在每个提交点
//! 留下一份副本，遍历副本就等于遍历全部崩溃点。遍历、逐点完备与点数对账在
//! `effects::matrix`，断言在 `support::checks`，场景在本文件。决策与局限见 ADR 0056、ADR 0062。

mod support;

use agent::EngineConfig;
use durable::EffectClass;
use effects::coverage::Coverage;
use effects::matrix::{Report, coverage_findings};
use support::matrix::{Call, Scenario, Step, run_scenario, walk};

/// 四个场景：纯文本、一个工具批次、多步与幂等工具，以及上下文超限后的压缩。
fn scenarios() -> Vec<Scenario> {
    vec![
        Scenario::new(
            "纯文本回答",
            vec![Step::Text("一句话回答".to_owned())],
            Vec::new(),
        ),
        tool_scenario(),
        Scenario::new(
            "多步与幂等工具",
            vec![
                Step::Calls(vec![
                    Call::new("call_read", "read_file"),
                    Call::new("call_write", "write_file"),
                    Call::new("call_edit", "edit_file"),
                    Call::new("call_bash", "bash"),
                ]),
                Step::Calls(vec![Call::new("call_read_again", "read_file")]),
                Step::Text("收尾".to_owned()),
            ],
            vec![
                ("call_read", true),
                ("call_write", true),
                ("call_edit", false),
                ("call_bash", false),
                ("call_read_again", true),
            ],
        ),
        compaction_scenario(),
    ]
}

/// 上下文超限后压缩并重试本步：两条消息让第二步的装配超过字符上限。
///
/// 压缩机会只取一次，因此重试一定成功；摘要由同一份脚本 provider 给出。
fn compaction_scenario() -> Scenario {
    Scenario {
        messages: vec!["第一问", "第二问"],
        config: EngineConfig {
            context_char_limit: 300,
            ..EngineConfig::default()
        },
        ..Scenario::new(
            "上下文超限后压缩",
            vec![
                Step::Text("答".repeat(200)),
                Step::Text("摘要：第一问与回答。".to_owned()),
                Step::Text("第二答".to_owned()),
            ],
            Vec::new(),
        )
    }
}

/// 一个工具批次：唯一一个真的会停在「已登记、未下发」的场景。
fn tool_scenario() -> Scenario {
    Scenario {
        expect_declared: true,
        ..Scenario::new(
            "一个工具批次",
            vec![
                Step::Calls(vec![Call::new("call_bash", "bash")]),
                Step::Text("工具之后的回答".to_owned()),
            ],
            vec![("call_bash", false)],
        )
    }
}

/// 规格 user story A-24：重启前的 running run 从树尾续跑。ADR 0056：每个提交点都成立。
///
/// 覆盖率在全部场景跑完之后自证：声明矩阵覆盖的类别必须真的出现过（ADR 0062）。
#[tokio::test]
async fn every_commit_prefix_recovers() {
    let directory = tempfile::tempdir().expect("创建临时目录");
    let mut report = Report::default();
    let coverage = Coverage::<EffectClass>::new();
    for (index, scenario) in scenarios().into_iter().enumerate() {
        let world = run_scenario(
            &directory.path().join(format!("scenario-{index}")),
            &scenario,
        )
        .await;
        for class in &world.declared {
            coverage.record(*class);
        }
        report.merge(walk(&scenario, &world).await);
    }
    report.extend(coverage_findings(&coverage));

    assert!(report.is_clean(), "{report}");
}

/// 工具节点已入树、帧尚未下发时，那个调用一定没有执行：恢复把它重跑一次，
/// 而不是把这次工作记成「结果未知」丢掉（ADR 0037、ADR 0056）。
#[tokio::test]
async fn undispatched_calls_are_rerun_instead_of_being_written_as_unknown() {
    let directory = tempfile::tempdir().expect("创建临时目录");
    let scenario = tool_scenario();
    let world = run_scenario(directory.path(), &scenario).await;
    let report = walk(&scenario, &world).await;

    assert!(report.is_clean(), "未下发的调用应当重跑一次：{report}");
}
