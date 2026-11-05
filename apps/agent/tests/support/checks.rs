//! 崩溃点矩阵的逐点断言：每条坐标给出一个结论。
//!
//! 坐标清单只有一处（[`PER_POINT`]），因此「少答一条」在报告里是一行「未应答」，而不是
//! 安静地少测一项。断言只返回结论，不 panic：一个点失败不能打断其它点（ADR 0062）。
//! 跨点才能得出的结论（至少一个点落在 run 中间、副本序号完整）在 `matrix::walk`。

use durable::{MessageRole, RunStatus, ToolResultState};
use effects::matrix::Checks;

use super::matrix::{Prefix, Restart, Scenario, World, tree_has};

/// 每个崩溃点都必须应答的坐标。新增一条断言就加在这里，报告会追着每个点要结论。
pub(crate) const PER_POINT: &[&str] = &[
    "收敛",
    "不变量",
    "历史保留",
    "没有 running 的 run",
    "没有执行中的工具",
    "节点归属",
    "非幂等至多一次",
    "重跑留下结局",
    "已登记即重跑",
];

/// 这个点没有读到：每条坐标各记一条失败，报告因此不会丢信息。
pub(crate) fn unavailable(detail: &str) -> Checks {
    let mut checks = Checks::new();
    for check in PER_POINT {
        checks.fail(check, format!("这个点没有读到：{detail}"));
    }
    checks
}

/// 一个点上的一组结论。
pub(crate) fn per_point(
    world: &World,
    scenario: &Scenario,
    prefix: &Prefix,
    recovered: &Restart,
) -> Checks {
    let mut checks = Checks::new();
    checks.record("收敛", converged(recovered));
    checks.record("不变量", invariants_empty(recovered));
    checks.record("历史保留", history_kept(recovered));
    checks.record("没有 running 的 run", no_running_run(recovered));
    checks.record("没有执行中的工具", no_unobserved_tool(recovered));
    checks.record("节点归属", node_ownership(recovered));
    checks.record(
        "非幂等至多一次",
        effect_limit(world, scenario, prefix, recovered),
    );
    checks.record("重跑留下结局", rerun_outcome(scenario, recovered));
    checks.record("已登记即重跑", declared_is_rerun(scenario, recovered));
    checks
}

/// 恢复后达到静止。早于 session 创建的副本没有 run 可跑，这一条自然成立。
fn converged(recovered: &Restart) -> Result<(), String> {
    let Some(tree) = &recovered.tree else {
        return Ok(());
    };
    if tree.session.busy {
        return Err("恢复后 session 仍在忙".to_owned());
    }
    if !tree.queue.is_empty() {
        return Err(format!("恢复后队列还剩 {} 项", tree.queue.len()));
    }
    Ok(())
}

/// 不变量一条都不许违例。
fn invariants_empty(recovered: &Restart) -> Result<(), String> {
    let Some(violation) = recovered.invariants.first() else {
        return Ok(());
    };
    Err(format!(
        "{} 条违例，第一条是 {}：{}",
        recovered.invariants.len(),
        violation.kind.as_str(),
        violation.detail
    ))
}

/// 崩溃前已经在链上的消息，重启后仍在链上。
fn history_kept(recovered: &Restart) -> Result<(), String> {
    let Some(tree) = &recovered.tree else {
        return Ok(());
    };
    for (id, _) in &recovered.before {
        if !tree_has(tree, *id) {
            return Err(format!("崩溃前的消息 {} 从链上消失了", id.get()));
        }
    }
    Ok(())
}

/// 没有停在 running 的 run。
fn no_running_run(recovered: &Restart) -> Result<(), String> {
    let Some(tree) = &recovered.tree else {
        return Ok(());
    };
    match tree
        .runs
        .iter()
        .find(|run| run.status == RunStatus::Running)
    {
        Some(run) => Err(format!("run {} 恢复后仍是 running", run.id.get())),
        None => Ok(()),
    }
}

/// 没有停在执行中的工具节点。
fn no_unobserved_tool(recovered: &Restart) -> Result<(), String> {
    match recovered
        .after
        .iter()
        .find(|(_, state)| state.is_unobserved())
    {
        Some((call_id, _)) => Err(format!("调用 {call_id} 停在执行中")),
        None => Ok(()),
    }
}

/// 每个节点要么挂在 run 上，要么是压缩消息。
fn node_ownership(recovered: &Restart) -> Result<(), String> {
    let Some(tree) = &recovered.tree else {
        return Ok(());
    };
    let stray = tree
        .nodes
        .iter()
        .find(|node| node.run_id.is_none() && node.role != MessageRole::Compaction);
    match stray {
        Some(node) => Err(format!(
            "消息 {} 既不挂在 run 上也不是压缩消息",
            node.id.get()
        )),
        None => Ok(()),
    }
}

/// 非幂等调用至多一次，幂等调用至多两次。上限的依据只有工具定义上的幂等标记。
fn effect_limit(
    world: &World,
    scenario: &Scenario,
    prefix: &Prefix,
    recovered: &Restart,
) -> Result<(), String> {
    for (call_id, idempotent) in &scenario.calls {
        let before = world.effects_before(call_id, prefix.serial);
        let reruns = recovered.reruns_of(call_id);
        let limit = if *idempotent { 2 } else { 1 };
        if before + reruns > limit {
            return Err(format!(
                "{call_id} 被执行了 {} 次（崩溃前 {before}、重启后 {reruns}），上限 {limit}",
                before + reruns
            ));
        }
    }
    Ok(())
}

/// 重跑过的调用必须留下结局，不能停在「结果未知」。
fn rerun_outcome(scenario: &Scenario, recovered: &Restart) -> Result<(), String> {
    for (call_id, _) in &scenario.calls {
        if recovered.reruns_of(call_id) == 0 {
            continue;
        }
        match recovered.state_of(call_id) {
            Some(ToolResultState::Finished { .. }) => {}
            other => {
                return Err(format!("{call_id} 被重跑却没有留下结局，实际是 {other:?}"));
            }
        }
    }
    Ok(())
}

/// 未下发的调用一定没有执行：恢复把它重跑一次并留下结局。其余的点自然成立。
fn declared_is_rerun(scenario: &Scenario, recovered: &Restart) -> Result<(), String> {
    for (call_id, _) in &scenario.calls {
        if recovered.state_before(call_id) != Some(&ToolResultState::Declared) {
            continue;
        }
        let reruns = recovered.reruns_of(call_id);
        if reruns != 1 {
            return Err(format!(
                "未下发的 {call_id} 重跑了 {reruns} 次，应当是 1 次"
            ));
        }
        match recovered.state_of(call_id) {
            Some(ToolResultState::Finished { .. }) => {}
            other => {
                return Err(format!("{call_id} 重跑之后应当有结局，实际是 {other:?}"));
            }
        }
    }
    Ok(())
}
