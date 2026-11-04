//! 恢复后的不变量检查：把「静默错乱」变成一份可断言的违例清单。
//! SQL 在 `crate::storage::sqlite::invariants`；调用点是 `RunEngine::round_zero` 与崩溃前缀矩阵。

use crate::error::StoreError;
use crate::storage::InvariantViolation;

use super::Store;

impl Store {
    /// 检查全部不变量。返回空表示当前库自洽。
    ///
    /// 断言的是「恢复之后、驱动 run 之前」的静止状态：一次崩溃之后，磁盘上只可能
    /// 出现这些形状里的某一种，而恢复的责任就是让它们全部消失（ADR 0037、ADR 0056）。
    ///
    /// # Errors
    ///
    /// - `StoreError::Database`：检查查询失败。
    pub async fn check_invariants(&self) -> Result<Vec<InvariantViolation>, StoreError> {
        self.storage.invariants().await
    }
}
