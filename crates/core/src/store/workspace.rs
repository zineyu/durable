//! workspace 的注册与移除。identity 是 host 与规范化绝对路径这一对。
//! session 的创建与删除在 `session`；事实事件定义在 `crate::event`；写命令入口在 `command`。

use super::Store;
use crate::error::StoreError;
use crate::event::FactEvent;
use crate::host::HostName;
use crate::id::WorkspacePath;
use crate::store::Command;
use crate::workspace::WorkspaceMetadata;

impl Store {
    /// 注册 workspace。
    ///
    /// 输入：已注册的 host 与路径身份。路径必须绝对且在该 host 上尚未登记。
    ///
    /// # Errors
    ///
    /// - `StoreError::HostNotFound`：该 host 未注册。
    /// - `StoreError::WorkspaceDuplicate`：该路径已在这台 host 上注册。
    /// - `StoreError::Database`：数据库读取或写入失败。
    /// - `StoreError::CorruptedData`：存储值不符合领域约束。
    /// - `StoreError::Serialization`：事实事件序列化失败。
    ///
    /// 副作用：写入 workspace 行与 `workspace_registered` 事实事件。
    pub async fn register_workspace(
        &self,
        host: &HostName,
        path: &WorkspacePath,
    ) -> Result<WorkspaceMetadata, StoreError> {
        self.command_root(async |command| {
            if self.storage.host(host).await?.is_none() {
                return Err(StoreError::HostNotFound(host.clone()));
            }
            if self.storage.workspace(host, path).await?.is_some() {
                return Err(StoreError::WorkspaceDuplicate(path.clone()));
            }
            Ok(insert_workspace(command, host, path))
        })
        .await
    }

    /// 注册启动参数给出的初始 workspace。
    ///
    /// 输入：`(host, path)` 列表，host 必须已注册；重复出现与已注册的路径被跳过。
    ///
    /// # Errors
    ///
    /// - `StoreError::HostNotFound`：任一条目的 host 未注册。
    /// - `StoreError::Database`：数据库读取或写入失败。
    /// - `StoreError::CorruptedData`：存储值不符合领域约束。
    /// - `StoreError::Serialization`：事实事件序列化失败。
    ///
    /// 副作用：为新路径写入 workspace 行与事实事件。整批一次提交。
    pub async fn register_initial_workspaces(
        &self,
        workspaces: &[(HostName, WorkspacePath)],
    ) -> Result<(), StoreError> {
        self.command_root(async |command| {
            let mut added: Vec<&(HostName, WorkspacePath)> = Vec::new();
            for workspace in workspaces {
                if self.storage.host(&workspace.0).await?.is_none() {
                    return Err(StoreError::HostNotFound(workspace.0.clone()));
                }
                if added.contains(&workspace)
                    || self
                        .storage
                        .workspace(&workspace.0, &workspace.1)
                        .await?
                        .is_some()
                {
                    continue;
                }
                added.push(workspace);
                insert_workspace(command, &workspace.0, &workspace.1);
            }
            Ok(())
        })
        .await
    }

    /// 移除 workspace 注册记录。
    ///
    /// 输入：已登记 host 与路径身份；不访问文件系统，因此目录已被删除时仍可移除。
    ///
    /// # Errors
    ///
    /// - `StoreError::WorkspaceNotFound`：该路径未在这台 host 上注册。
    /// - `StoreError::WorkspaceNotEmpty`：该 workspace 仍有 session。
    /// - `StoreError::Database`：数据库读取或写入失败。
    /// - `StoreError::Serialization`：事实事件序列化失败。
    ///
    /// 副作用：删除 workspace 行，写入 `workspace_removed` 事实事件。
    pub async fn remove_workspace(
        &self,
        host: &HostName,
        path: &WorkspacePath,
    ) -> Result<(), StoreError> {
        self.command_root(async |command| {
            if self.storage.workspace(host, path).await?.is_none() {
                return Err(StoreError::WorkspaceNotFound(path.clone()));
            }
            if self.storage.session_count(host, path).await? > 0 {
                return Err(StoreError::WorkspaceNotEmpty(path.clone()));
            }
            let now = command.now();
            command
                .changes()
                .delete_workspace(host.clone(), path.clone());
            command.changes().append_event(
                FactEvent::WorkspaceRemoved {
                    host: host.clone(),
                    path: path.clone(),
                },
                now,
            );
            Ok(())
        })
        .await
    }
}

/// 登记一次 workspace 插入，并返回它的元数据。
fn insert_workspace(
    command: &mut Command<'_>,
    host: &HostName,
    path: &WorkspacePath,
) -> WorkspaceMetadata {
    let registered_at = command.now();
    let workspace = WorkspaceMetadata {
        host: host.clone(),
        path: path.clone(),
        registered_at,
    };
    command.changes().insert_workspace(workspace.clone());
    command.changes().append_event(
        FactEvent::WorkspaceRegistered {
            workspace: workspace.clone(),
        },
        registered_at,
    );
    workspace
}
