//! 恢复后不变量的 SQL：每条查询返回一类违例。
//! 调用点在 `store/invariants`；命令与迁移在父模块与 `apply`，读路径在 `message`。

use sqlx::Row;

use super::SqliteStorage;
use crate::error::StoreError;
use crate::storage::{InvariantKind, InvariantViolation};

impl SqliteStorage {
    /// 全部不变量检查。返回空表示当前库自洽。
    pub(crate) async fn invariants(&self) -> Result<Vec<InvariantViolation>, StoreError> {
        let mut violations = Vec::new();
        violations.extend(self.running_runs_per_session().await?);
        violations.extend(self.progress_on_finished_run().await?);
        violations.extend(self.running_tool_without_running_run().await?);
        violations.extend(self.duplicate_tool_result().await?);
        violations.extend(self.unpaired_tool_result().await?);
        violations.extend(self.active_leaf_off_tree().await?);
        violations.extend(self.queue_anchor_off_tree().await?);
        violations.extend(self.allocator_behind().await?);
        violations.extend(self.terminal_run_missing_tool_result().await?);
        Ok(violations)
    }

    async fn running_runs_per_session(&self) -> Result<Vec<InvariantViolation>, StoreError> {
        let rows = sqlx::query(
            "SELECT session_id, COUNT(*) AS runs FROM run
             WHERE status = 'running' GROUP BY session_id HAVING COUNT(*) > 1",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|row| InvariantViolation {
                kind: InvariantKind::RunningRunPerSession,
                detail: format!(
                    "session {} 有 {} 条 running 的 run",
                    row.get::<String, _>("session_id"),
                    row.get::<i64, _>("runs")
                ),
            })
            .collect())
    }

    async fn progress_on_finished_run(&self) -> Result<Vec<InvariantViolation>, StoreError> {
        let rows = sqlx::query(
            "SELECT id, status FROM run WHERE progress IS NOT NULL AND status <> 'running'",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|row| InvariantViolation {
                kind: InvariantKind::ProgressOnFinishedRun,
                detail: format!(
                    "run {} 已结束（{}）却留着 progress",
                    row.get::<i64, _>("id"),
                    row.get::<String, _>("status")
                ),
            })
            .collect())
    }

    async fn running_tool_without_running_run(
        &self,
    ) -> Result<Vec<InvariantViolation>, StoreError> {
        let rows = sqlx::query(
            "SELECT m.session_id AS session_id, p.message_id AS message_id
             FROM message_part p JOIN message m ON m.id = p.message_id
             WHERE p.kind = 'tool_result' AND p.state = 'running'
               AND NOT EXISTS (SELECT 1 FROM run r
                               WHERE r.session_id = m.session_id AND r.status = 'running')",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|row| InvariantViolation {
                kind: InvariantKind::RunningToolWithoutRunningRun,
                detail: format!(
                    "session {} 的工具节点 {} 仍在执行中，但该 session 没有 running 的 run",
                    row.get::<String, _>("session_id"),
                    row.get::<i64, _>("message_id")
                ),
            })
            .collect())
    }

    async fn duplicate_tool_result(&self) -> Result<Vec<InvariantViolation>, StoreError> {
        let rows = sqlx::query(
            "SELECT m.session_id AS session_id, json_extract(p.body, '$.callId') AS call_id,
                    COUNT(*) AS count
             FROM message_part p JOIN message m ON m.id = p.message_id
             WHERE p.kind = 'tool_result'
             GROUP BY m.session_id, call_id HAVING COUNT(*) > 1",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|row| InvariantViolation {
                kind: InvariantKind::DuplicateToolResult,
                detail: format!(
                    "session {} 的调用 {} 有 {} 个工具结局节点",
                    row.get::<String, _>("session_id"),
                    row.get::<String, _>("call_id"),
                    row.get::<i64, _>("count")
                ),
            })
            .collect())
    }

    async fn unpaired_tool_result(&self) -> Result<Vec<InvariantViolation>, StoreError> {
        let rows = sqlx::query(
            "SELECT m.session_id AS session_id, p.message_id AS message_id,
                    json_extract(p.body, '$.callId') AS call_id
             FROM message_part p JOIN message m ON m.id = p.message_id
             WHERE p.kind = 'tool_result' AND NOT EXISTS (
                 SELECT 1 FROM message_part c JOIN message mc ON mc.id = c.message_id
                 WHERE c.kind = 'tool_call' AND mc.session_id = m.session_id
                   AND json_extract(c.body, '$.callId') = json_extract(p.body, '$.callId'))",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|row| InvariantViolation {
                kind: InvariantKind::UnpairedToolResult,
                detail: format!(
                    "session {} 的工具节点 {} 声称是调用 {} 的结局，但该调用不在任何 assistant 消息里",
                    row.get::<String, _>("session_id"),
                    row.get::<i64, _>("message_id"),
                    row.get::<String, _>("call_id")
                ),
            })
            .collect())
    }

    async fn active_leaf_off_tree(&self) -> Result<Vec<InvariantViolation>, StoreError> {
        let rows = sqlx::query(
            "SELECT s.id AS id, s.active_leaf AS active_leaf FROM session s
             WHERE s.active_leaf IS NOT NULL
               AND NOT EXISTS (SELECT 1 FROM message m
                               WHERE m.session_id = s.id AND m.id = s.active_leaf)",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|row| InvariantViolation {
                kind: InvariantKind::ActiveLeafOffTree,
                detail: format!(
                    "session {} 的 active leaf {} 不在消息表里",
                    row.get::<String, _>("id"),
                    row.get::<i64, _>("active_leaf")
                ),
            })
            .collect())
    }

    async fn queue_anchor_off_tree(&self) -> Result<Vec<InvariantViolation>, StoreError> {
        let rows = sqlx::query(
            "SELECT q.session_id AS session_id, q.id AS id, q.anchor AS anchor FROM queue_item q
             WHERE q.anchor IS NOT NULL
               AND NOT EXISTS (SELECT 1 FROM message m
                               WHERE m.session_id = q.session_id AND m.id = q.anchor)",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|row| InvariantViolation {
                kind: InvariantKind::QueueAnchorOffTree,
                detail: format!(
                    "session {} 的队列项 {} 锚定到不存在的消息 {}",
                    row.get::<String, _>("session_id"),
                    row.get::<i64, _>("id"),
                    row.get::<i64, _>("anchor")
                ),
            })
            .collect())
    }

    async fn allocator_behind(&self) -> Result<Vec<InvariantViolation>, StoreError> {
        let row = sqlx::query(
            "SELECT (SELECT value FROM meta WHERE key = 'next_message_id') AS next_message,
                    (SELECT COALESCE(MAX(id), 0) FROM message) AS max_message,
                    (SELECT value FROM meta WHERE key = 'next_run_id') AS next_run,
                    (SELECT COALESCE(MAX(id), 0) FROM run) AS max_run,
                    (SELECT value FROM meta WHERE key = 'next_queue_item_id') AS next_queue,
                    (SELECT COALESCE(MAX(id), 0) FROM queue_item) AS max_queue",
        )
        .fetch_one(&self.pool)
        .await?;
        let mut violations = Vec::new();
        for (name, next, max) in [
            (
                "next_message_id",
                row.get::<i64, _>("next_message"),
                row.get::<i64, _>("max_message"),
            ),
            (
                "next_run_id",
                row.get::<i64, _>("next_run"),
                row.get::<i64, _>("max_run"),
            ),
            (
                "next_queue_item_id",
                row.get::<i64, _>("next_queue"),
                row.get::<i64, _>("max_queue"),
            ),
        ] {
            if next <= max {
                violations.push(InvariantViolation {
                    kind: InvariantKind::AllocatorBehind,
                    detail: format!("{name} 是 {next}，但表里已有 id {max}，下次铸造会撞车"),
                });
            }
        }
        Ok(violations)
    }

    async fn terminal_run_missing_tool_result(
        &self,
    ) -> Result<Vec<InvariantViolation>, StoreError> {
        let rows = sqlx::query(
            "SELECT m.session_id AS session_id, m.id AS message_id,
                    json_extract(c.body, '$.callId') AS call_id
             FROM message_part c
             JOIN message m ON m.id = c.message_id
             JOIN run r ON r.id = m.run_id
             WHERE c.kind = 'tool_call' AND r.status <> 'running'
               AND NOT EXISTS (
                   SELECT 1 FROM message_part p JOIN message x ON x.id = p.message_id
                   WHERE p.kind = 'tool_result' AND x.session_id = m.session_id
                     AND json_extract(p.body, '$.callId') = json_extract(c.body, '$.callId'))",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|row| InvariantViolation {
                kind: InvariantKind::TerminalRunMissingToolResult,
                detail: format!(
                    "session {} 的已结束 run 里，消息 {} 声明的调用 {} 没有工具结局",
                    row.get::<String, _>("session_id"),
                    row.get::<i64, _>("message_id"),
                    row.get::<String, _>("call_id")
                ),
            })
            .collect())
    }
}
