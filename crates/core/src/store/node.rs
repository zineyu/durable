//! 节点表：注册、改名、删除与读取。
//! hub 侧的节点状态与上游订阅在 `crates/hub/src/bus`。

use super::{Store, now_millis};
use crate::change::Changeset;
use crate::error::StoreError;
use crate::node::{NodeMetadata, NodeName};

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
        let node = NodeMetadata {
            name,
            url,
            created_at: now_millis(),
        };
        let mut changes = Changeset::new();
        changes.insert_node(node.clone());
        self.commit(&changes).await?;
        Ok(node)
    }

    /// 重命名节点。
    ///
    /// 输入：节点表中的旧名与新名。名称即身份，改名后旧名不再存在。
    ///
    /// # Errors
    ///
    /// - `StoreError::NodeNotFound`：旧名不在节点表中。
    /// - `StoreError::NodeNameTaken`：新名已被使用。
    /// - `StoreError::Database`：数据库读取或写入失败。
    /// - `StoreError::CorruptedData`：存储值不符合领域约束。
    ///
    /// 副作用：替换 node 行，保留 URL 与注册时间。
    pub async fn rename_node(
        &self,
        name: &NodeName,
        next: NodeName,
    ) -> Result<NodeMetadata, StoreError> {
        let _line = self.line.lock().await;
        let Some(node) = self.storage.node(name).await? else {
            return Err(StoreError::NodeNotFound(name.clone()));
        };
        if *name == next {
            return Ok(node);
        }
        if self.storage.node(&next).await?.is_some() {
            return Err(StoreError::NodeNameTaken(next));
        }
        let renamed = NodeMetadata {
            name: next,
            url: node.url,
            created_at: node.created_at,
        };
        let mut changes = Changeset::new();
        changes.delete_node(name.clone());
        changes.insert_node(renamed.clone());
        self.commit(&changes).await?;
        Ok(renamed)
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
