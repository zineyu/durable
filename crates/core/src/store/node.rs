//! 节点表：注册、更新、删除与读取。
//! hub 侧的节点状态与上游订阅在 `crates/hub/src/bus`。

use super::{Store, now_millis};
use crate::change::Changeset;
use crate::error::StoreError;
use crate::node::{NodeMetadata, NodeName, NodeUpdate};

impl Store {
    /// 节点表中的单个节点。
    ///
    /// # Errors
    ///
    /// - `StoreError::Database`：数据库读取失败。
    /// - `StoreError::CorruptedData`：存储值不符合领域约束。
    pub async fn node(&self, name: &NodeName) -> Result<Option<NodeMetadata>, StoreError> {
        self.storage.node(name).await
    }

    /// 节点表中的全部节点，按注册顺序排列。
    ///
    /// # Errors
    ///
    /// - `StoreError::Database`：数据库读取失败。
    /// - `StoreError::CorruptedData`：存储值不符合领域约束。
    pub async fn nodes(&self) -> Result<Vec<NodeMetadata>, StoreError> {
        self.storage.nodes().await
    }

    /// 注册节点。
    ///
    /// 输入：已解析的节点名与非空 URL。
    ///
    /// # Errors
    ///
    /// - `StoreError::NodeNameTaken`：该名称已注册。
    /// - `StoreError::Database`：数据库读取或写入失败。
    /// - `StoreError::CorruptedData`：存储值不符合领域约束。
    ///
    /// 副作用：写入 node 行。
    pub async fn register_node(
        &self,
        name: NodeName,
        url: String,
    ) -> Result<NodeMetadata, StoreError> {
        let _line = self.line.lock().await;
        if self.storage.node(&name).await?.is_some() {
            return Err(StoreError::NodeNameTaken(name));
        }
        let now = now_millis();
        let node = NodeMetadata {
            name,
            url,
            enabled: true,
            created_at: now,
            updated_at: now,
        };
        let mut changes = Changeset::new();
        changes.insert_node(node.clone());
        self.commit(&changes).await?;
        Ok(node)
    }

    /// 更新节点。`update` 中只处理给出的字段，其余保留原值。
    ///
    /// 输入：节点表中的当前名与局部更新。名称即身份，改名后旧名不再存在。
    ///
    /// # Errors
    ///
    /// - `StoreError::NodeNotFound`：当前名不在节点表中。
    /// - `StoreError::NodeNameTaken`：改名后的新名已被使用。
    /// - `StoreError::Database`：数据库读取或写入失败。
    /// - `StoreError::CorruptedData`：存储值不符合领域约束。
    ///
    /// 副作用：写入 node 行。字段与原值完全相同则不写库、不推进 `updated_at`。
    pub async fn update_node(
        &self,
        name: &NodeName,
        update: NodeUpdate,
    ) -> Result<NodeMetadata, StoreError> {
        let _line = self.line.lock().await;
        let Some(node) = self.storage.node(name).await? else {
            return Err(StoreError::NodeNotFound(name.clone()));
        };
        let next_name = update.name.unwrap_or_else(|| node.name.clone());
        let url = update.url.unwrap_or_else(|| node.url.clone());
        let enabled = update.enabled.unwrap_or(node.enabled);
        if next_name == node.name && url == node.url && enabled == node.enabled {
            return Ok(node);
        }
        if next_name != node.name && self.storage.node(&next_name).await?.is_some() {
            return Err(StoreError::NodeNameTaken(next_name));
        }
        let updated = NodeMetadata {
            name: next_name,
            url,
            enabled,
            created_at: node.created_at,
            updated_at: now_millis(),
        };
        let mut changes = Changeset::new();
        if updated.name == node.name {
            changes.update_node(updated.clone());
        } else {
            changes.delete_node(node.name);
            changes.insert_node(updated.clone());
        }
        self.commit(&changes).await?;
        Ok(updated)
    }

    /// 移除节点。
    ///
    /// # Errors
    ///
    /// - `StoreError::NodeNotFound`：该名称不在节点表中。
    /// - `StoreError::Database`：数据库读取或写入失败。
    ///
    /// 副作用：删除 node 行。
    pub async fn remove_node(&self, name: &NodeName) -> Result<(), StoreError> {
        let _line = self.line.lock().await;
        if self.storage.node(name).await?.is_none() {
            return Err(StoreError::NodeNotFound(name.clone()));
        }
        let mut changes = Changeset::new();
        changes.delete_node(name.clone());
        self.commit(&changes).await.map(|_| ())
    }
}
