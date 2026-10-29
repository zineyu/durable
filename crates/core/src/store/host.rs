//! host 表：注册、更新、删除、读取与状态事实。
//! host 的运行状态与 link 在 `crates/agent`；写命令入口在 `command`。

use super::Store;
use crate::error::StoreError;
use crate::event::FactEvent;
use crate::host::{HostKind, HostMetadata, HostName, HostStatus, HostUpdate};

impl Store {
    /// host 表中的单个 host。
    ///
    /// # Errors
    ///
    /// - `StoreError::Database`：数据库读取失败。
    /// - `StoreError::CorruptedData`：存储值不符合领域约束。
    pub async fn host(&self, name: &HostName) -> Result<Option<HostMetadata>, StoreError> {
        self.storage.host(name).await
    }

    /// host 表中的全部 host，按注册顺序排列。
    ///
    /// # Errors
    ///
    /// - `StoreError::Database`：数据库读取失败。
    /// - `StoreError::CorruptedData`：存储值不符合领域约束。
    pub async fn hosts(&self) -> Result<Vec<HostMetadata>, StoreError> {
        self.storage.hosts().await
    }

    /// 注册 host。
    ///
    /// 输入：已解析的 host 名、种类与可选 SSH 目标。`ssh` 种类必须给出目标。
    ///
    /// # Errors
    ///
    /// - `StoreError::HostNameTaken`：该名称已注册。
    /// - `StoreError::Database`：数据库读取或写入失败。
    /// - `StoreError::Serialization`：事实事件序列化失败。
    ///
    /// 副作用：写入 host 行与 `host_registered` 事实事件。
    pub async fn register_host(
        &self,
        name: HostName,
        kind: HostKind,
        target: Option<String>,
    ) -> Result<HostMetadata, StoreError> {
        self.command_root(async |command| {
            if self.storage.host(&name).await?.is_some() {
                return Err(StoreError::HostNameTaken(name));
            }
            let now = command.now();
            let host = HostMetadata {
                name,
                kind,
                target,
                enabled: true,
                created_at: now,
                updated_at: now,
            };
            command.changes().insert_host(host.clone());
            command
                .changes()
                .append_event(FactEvent::HostRegistered { host: host.clone() }, now);
            Ok(host)
        })
        .await
    }

    /// 更新 host。`update` 中只处理给出的字段，其余保留原值。
    ///
    /// 输入：host 表中的当前名与局部更新。名称即身份，改名后旧名不再存在。
    ///
    /// # Errors
    ///
    /// - `StoreError::HostNotFound`：当前名不在 host 表中。
    /// - `StoreError::HostNameTaken`：改名后的新名已被使用。
    /// - `StoreError::Database`：数据库读取或写入失败。
    /// - `StoreError::CorruptedData`：存储值不符合领域约束。
    /// - `StoreError::Serialization`：事实事件序列化失败。
    ///
    /// 副作用：写入 host 行与 `host_updated` 事实事件。字段与原值完全相同则不写库、
    /// 不推进 `updated_at`，也不写事实。
    pub async fn update_host(
        &self,
        name: &HostName,
        update: HostUpdate,
    ) -> Result<HostMetadata, StoreError> {
        self.command_root(async |command| {
            let Some(host) = self.storage.host(name).await? else {
                return Err(StoreError::HostNotFound(name.clone()));
            };
            let next_name = update.name.unwrap_or_else(|| host.name.clone());
            let target = update.target.or_else(|| host.target.clone());
            let enabled = update.enabled.unwrap_or(host.enabled);
            if next_name == host.name && target == host.target && enabled == host.enabled {
                return Ok(host);
            }
            if next_name != host.name && self.storage.host(&next_name).await?.is_some() {
                return Err(StoreError::HostNameTaken(next_name));
            }
            let now = command.now();
            let updated = HostMetadata {
                name: next_name,
                kind: host.kind,
                target,
                enabled,
                created_at: host.created_at,
                updated_at: now,
            };
            if updated.name == host.name {
                command.changes().update_host(updated.clone());
            } else {
                command.changes().delete_host(host.name);
                command.changes().insert_host(updated.clone());
            }
            command.changes().append_event(
                FactEvent::HostUpdated {
                    host: updated.clone(),
                },
                now,
            );
            Ok(updated)
        })
        .await
    }

    /// 移除 host。仍有 workspace 时拒绝：删登记不该连带丢掉 workspace 与 session。
    ///
    /// # Errors
    ///
    /// - `StoreError::HostNotFound`：该名称不在 host 表中。
    /// - `StoreError::HostNotEmpty`：该 host 仍登记有 workspace。
    /// - `StoreError::Database`：数据库读取或写入失败。
    /// - `StoreError::Serialization`：事实事件序列化失败。
    ///
    /// 副作用：删除 host 行与 `host_removed` 事实事件。
    pub async fn remove_host(&self, name: &HostName) -> Result<(), StoreError> {
        self.command_root(async |command| {
            if self.storage.host(name).await?.is_none() {
                return Err(StoreError::HostNotFound(name.clone()));
            }
            let workspaces = self.storage.host_workspace_count(name).await?;
            if workspaces > 0 {
                return Err(StoreError::HostNotEmpty(name.clone(), workspaces));
            }
            let now = command.now();
            command.changes().delete_host(name.clone());
            command
                .changes()
                .append_event(FactEvent::HostRemoved { host: name.clone() }, now);
            Ok(())
        })
        .await
    }

    /// 宣告 host 的运行状态。状态本身是进程状态，不落库，只有事实落库。
    ///
    /// # Errors
    ///
    /// - `StoreError::HostNotFound`：该名称不在 host 表中。
    /// - `StoreError::Database`：数据库读写失败。
    /// - `StoreError::Serialization`：事实事件序列化失败。
    ///
    /// 副作用：写入 `host_status_changed` 事实事件。
    pub async fn announce_host_status(
        &self,
        name: &HostName,
        status: HostStatus,
        detail: Option<String>,
    ) -> Result<(), StoreError> {
        self.command_root(async |command| {
            if self.storage.host(name).await?.is_none() {
                return Err(StoreError::HostNotFound(name.clone()));
            }
            let now = command.now();
            command.changes().append_event(
                FactEvent::HostStatusChanged {
                    host: name.clone(),
                    status,
                    detail,
                },
                now,
            );
            Ok(())
        })
        .await
    }
}
