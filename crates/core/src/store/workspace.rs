//! workspace 的注册与移除。
//! session 的创建与删除在 `session`；事实事件定义在 `crate::event`。

use super::{Store, now_millis};
use crate::change::Changeset;
use crate::error::StoreError;
use crate::event::FactEvent;
use crate::id::WorkspacePath;
use crate::workspace::WorkspaceMetadata;

impl Store {
    /// 注册 workspace。
    ///
    /// 输入：已规范化的 workspace 身份路径。
    ///
    /// # Errors
    ///
    /// - `StoreError::WorkspaceDuplicate`：该路径已注册。
    /// - `StoreError::Database`：数据库读取或写入失败。
    /// - `StoreError::CorruptedData`：存储值不符合领域约束。
    /// - `StoreError::Serialization`：事实事件序列化失败。
    ///
    /// 副作用：写入 workspace 行与 `workspace_registered` 事实事件。
    pub async fn register_workspace(
        &self,
        path: &WorkspacePath,
    ) -> Result<WorkspaceMetadata, StoreError> {
        let _line = self.line.lock().await;
        if self.storage.workspace(path).await?.is_some() {
            return Err(StoreError::WorkspaceDuplicate(path.clone()));
        }
        self.insert_workspace(path).await
    }

    /// 注册启动参数给出的初始 workspace。
    ///
    /// 输入：已规范化的 workspace 路径列表；重复出现与已注册的路径被跳过。
    ///
    /// # Errors
    ///
    /// - `StoreError::Database`：数据库读取或写入失败。
    /// - `StoreError::CorruptedData`：存储值不符合领域约束。
    /// - `StoreError::Serialization`：事实事件序列化失败。
    ///
    /// 副作用：为新路径写入 workspace 行与事实事件。
    pub async fn register_initial_workspaces(
        &self,
        paths: &[WorkspacePath],
    ) -> Result<(), StoreError> {
        let _line = self.line.lock().await;
        for path in paths {
            if self.storage.workspace(path).await?.is_some() {
                continue;
            }
            self.insert_workspace(path).await?;
        }
        Ok(())
    }

    async fn insert_workspace(
        &self,
        path: &WorkspacePath,
    ) -> Result<WorkspaceMetadata, StoreError> {
        let registered_at = now_millis();
        let workspace = WorkspaceMetadata {
            path: path.clone(),
            registered_at,
        };
        let mut changes = Changeset::new();
        changes.insert_workspace(workspace.clone());
        changes.append_event(
            FactEvent::WorkspaceRegistered {
                workspace: workspace.clone(),
            },
            registered_at,
        );
        self.commit(&changes).await?;
        Ok(workspace)
    }

    /// 移除 workspace 注册记录。
    ///
    /// 输入：存储中的 workspace 身份路径；不访问文件系统，因此目录已被删除时仍可移除。
    ///
    /// # Errors
    ///
    /// - `StoreError::WorkspaceNotFound`：该路径未注册。
    /// - `StoreError::WorkspaceNotEmpty`：该 workspace 仍有 session。
    /// - `StoreError::Database`：数据库读取或写入失败。
    /// - `StoreError::Serialization`：事实事件序列化失败。
    ///
    /// 副作用：删除 workspace 行，写入 `workspace_removed` 事实事件。
    pub async fn remove_workspace(&self, path: &WorkspacePath) -> Result<(), StoreError> {
        let _line = self.line.lock().await;
        if self.storage.workspace(path).await?.is_none() {
            return Err(StoreError::WorkspaceNotFound(path.clone()));
        }
        if self.storage.session_count(path).await? > 0 {
            return Err(StoreError::WorkspaceNotEmpty(path.clone()));
        }
        let mut changes = Changeset::new();
        changes.delete_workspace(path.clone());
        changes.append_event(
            FactEvent::WorkspaceRemoved { path: path.clone() },
            now_millis(),
        );
        self.commit(&changes).await.map(|_| ())
    }
}
