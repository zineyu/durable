//! session 的创建、改名、删除与读取。
//! 队列写入在 `queue`，run 生命周期在 `run`，分支命令在 `branch`，写命令入口在 `command`。

use std::collections::HashMap;

use super::{Store, snapshot_into_tree};
use crate::error::StoreError;
use crate::event::FactEvent;
use crate::id::{MessageId, SessionId, SessionTitle, WorkspacePath};
use crate::message::{ContentPart, MessageBody, MessageNode, MessageSkeleton};
use crate::session::{SessionMetadata, SessionRecord, SessionTreeSnapshot};
use crate::storage::SessionSnapshot;

impl Store {
    /// 在指定 workspace 下创建 session。
    ///
    /// 输入：已规范化的 workspace 路径与已解析的标题。
    ///
    /// # Errors
    ///
    /// - `StoreError::WorkspaceNotFound`：该 workspace 未注册。
    /// - `StoreError::Database`：数据库读取或写入失败。
    /// - `StoreError::CorruptedData`：存储值不符合领域约束。
    /// - `StoreError::Serialization`：事实事件序列化失败。
    ///
    /// 副作用：写入 session 行与 `session_created` 事实事件。
    pub async fn create_session(
        &self,
        workspace: &WorkspacePath,
        title: SessionTitle,
    ) -> Result<SessionMetadata, StoreError> {
        self.command_root(async |command| {
            if self.storage.workspace(workspace).await?.is_none() {
                return Err(StoreError::WorkspaceNotFound(workspace.clone()));
            }
            let now = command.now();
            let session = SessionRecord {
                id: SessionId::generate(),
                workspace: workspace.clone(),
                title: title.clone(),
                title_auto: true,
                created_at: now,
                updated_at: now,
                active_leaf: None,
            };
            let view = command.insert_session(session.clone());
            command.changes().append_event(
                FactEvent::SessionCreated {
                    session_id: session.id.clone(),
                },
                now,
            );
            Ok(view)
        })
        .await
    }

    /// 重命名 session。
    ///
    /// 输入：session 身份与已解析的标题。
    ///
    /// # Errors
    ///
    /// - `StoreError::SessionNotFound`：session 不存在。
    /// - `StoreError::Database`：数据库读取或写入失败。
    /// - `StoreError::CorruptedData`：存储值不符合领域约束。
    /// - `StoreError::Serialization`：事实事件序列化失败。
    ///
    /// 副作用：标题变化时更新标题与 `updated_at`，写入 `session_renamed` 事实事件。
    pub async fn rename_session(
        &self,
        id: &SessionId,
        title: &SessionTitle,
    ) -> Result<SessionMetadata, StoreError> {
        self.command(id, async |command| {
            command.session_mut().title = title.clone();
            command.session_mut().title_auto = false;
            let view = command.view()?;
            let now = command.now();
            command.changes().append_event(
                FactEvent::SessionRenamed {
                    session_id: id.clone(),
                },
                now,
            );
            Ok(view)
        })
        .await
    }

    /// 删除 session。
    ///
    /// 输入：session 身份。
    ///
    /// # Errors
    ///
    /// - `StoreError::SessionNotFound`：session 不存在。
    /// - `StoreError::Database`：数据库读取或写入失败。
    /// - `StoreError::Serialization`：事实事件序列化失败。
    ///
    /// 副作用：删除该 session 的消息树、队列、run 与历史事件，再写入 `session_deleted` 事实事件。
    /// 不访问 workspace 文件系统。
    pub async fn delete_session(&self, id: &SessionId) -> Result<(), StoreError> {
        self.command(id, async |command| {
            let now = command.now();
            command.changes().delete_session(id.clone());
            command.changes().append_event(
                FactEvent::SessionDeleted {
                    session_id: id.clone(),
                },
                now,
            );
            Ok(())
        })
        .await
    }

    /// 读取单个 session 的消息树。
    ///
    /// # Errors
    ///
    /// - `StoreError::SessionNotFound`：session 不存在。
    /// - `StoreError::Database`：数据库读取失败。
    /// - `StoreError::CorruptedData`：存储值不符合领域约束。
    pub async fn session_tree(&self, id: &SessionId) -> Result<SessionTreeSnapshot, StoreError> {
        self.storage
            .session_tree(id)
            .await?
            .map(snapshot_into_tree)
            .ok_or_else(|| StoreError::SessionNotFound(id.clone()))
    }

    /// 读取 session 的 transcript：root 到 active leaf 的路径，到最近一条压缩消息为止。
    ///
    /// 链上顺序与截断由 [`crate::tree::MessageTree`] 回答，正文只按链上的 id 取。
    ///
    /// # Errors
    ///
    /// - `StoreError::SessionNotFound`：session 不存在。
    /// - `StoreError::Database`：数据库读取失败。
    /// - `StoreError::CorruptedData`：存储值不符合领域约束。
    pub async fn transcript(&self, id: &SessionId) -> Result<Vec<MessageNode>, StoreError> {
        let session = self.find_session(id).await?;
        let Some(leaf) = session.active_leaf else {
            return Ok(Vec::new());
        };
        let chain: Vec<MessageSkeleton> = self.message_tree(id).await?.transcript(leaf);
        let ids: Vec<MessageId> = chain.iter().map(|node| node.id).collect();
        let mut parts: HashMap<MessageId, Vec<ContentPart>> = self
            .storage
            .message_bodies(id, &ids)
            .await?
            .into_iter()
            .map(|body| (body.id, body.parts))
            .collect();
        Ok(chain
            .into_iter()
            .map(|node| {
                let body = parts.remove(&node.id).unwrap_or_default();
                node.into_node(body)
            })
            .collect())
    }

    /// 按消息 id 批量读取块正文。
    ///
    /// # Errors
    ///
    /// - `StoreError::SessionNotFound`：session 不存在。
    /// - `StoreError::Database`：数据库读取失败。
    /// - `StoreError::CorruptedData`：存储值不符合领域约束。
    pub async fn message_bodies(
        &self,
        id: &SessionId,
        ids: &[MessageId],
    ) -> Result<Vec<MessageBody>, StoreError> {
        self.find_session(id).await?;
        self.storage.message_bodies(id, ids).await
    }

    pub(super) async fn session_snapshot(
        &self,
        id: &SessionId,
    ) -> Result<SessionSnapshot, StoreError> {
        self.storage
            .session_tree(id)
            .await?
            .ok_or_else(|| StoreError::SessionNotFound(id.clone()))
    }

    /// 需要执行循环的 session：有 running 的 run 或队列非空。
    ///
    /// # Errors
    ///
    /// - `StoreError::Database`：数据库读取失败。
    /// - `StoreError::CorruptedData`：存储值不符合领域约束。
    pub async fn sessions_with_work(&self) -> Result<Vec<SessionId>, StoreError> {
        self.storage.sessions_with_work().await
    }

    /// 队列非空的全部 session。
    ///
    /// # Errors
    ///
    /// - `StoreError::Database`：数据库读取失败。
    /// - `StoreError::CorruptedData`：存储值不符合领域约束。
    pub async fn sessions_with_queue(&self) -> Result<Vec<SessionId>, StoreError> {
        self.storage.sessions_with_queue().await
    }

    /// 读取 session 记录。
    ///
    /// # Errors
    ///
    /// - `StoreError::SessionNotFound`：session 不存在。
    /// - `StoreError::Database`：数据库读取失败。
    /// - `StoreError::CorruptedData`：存储值不符合领域约束。
    pub async fn session_record(&self, id: &SessionId) -> Result<SessionRecord, StoreError> {
        self.find_session(id).await
    }

    pub(super) async fn find_session(&self, id: &SessionId) -> Result<SessionRecord, StoreError> {
        self.storage
            .session(id)
            .await?
            .ok_or_else(|| StoreError::SessionNotFound(id.clone()))
    }
}
