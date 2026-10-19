use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::id::WorkspacePath;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceMetadata {
    pub path: WorkspacePath,
    #[ts(type = "number")]
    pub registered_at: i64,
}
