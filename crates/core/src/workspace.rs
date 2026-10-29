//! workspace 元数据：identity 是 host 与规范化绝对路径这一对，另有注册时间。
//! 路径校验在 host 上的 executor 完成，`id` 只判语法。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::host::HostName;
use crate::id::WorkspacePath;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceMetadata {
    /// workspace 所属的 host。工具调用固定走它的 link。
    pub host: HostName,
    pub path: WorkspacePath,
    #[ts(type = "number")]
    pub registered_at: i64,
}
