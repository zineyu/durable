//! session 的创建、改名、删除与读取，以及分支命令共用的空闲判定。
//! 队列写入在 `queue`，run 生命周期在 `run`，分支命令在 `branch`。

use super::{Store, now_millis, path_to_leaf, snapshot_into_tree};
use crate::change::Changeset;
use crate::error::StoreError;
use crate::event::FactEvent;
use crate::id::{SessionId, SessionTitle, WorkspacePath};
use crate::message::MessageNode;
use crate::session::{SessionMetadata, SessionRecord, SessionTree};
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
        let _line = self.line.lock().await;
        if self.storage.workspace(workspace).await?.is_none() {
            return Err(StoreError::WorkspaceNotFound(workspace.clone()));
        }
        let created_at = now_millis();
        let session = SessionRecord {
            id: SessionId::generate(),
            workspace: workspace.clone(),
            title,
            title_auto: true,
            created_at,
            updated_at: created_at,
            active_leaf: None,
        };
        let mut changes = Changeset::new();
        changes.insert_session(session.clone());
        changes.append_event(
            FactEvent::SessionCreated {
                session: session.metadata(0, false),
            },
            created_at,
        );
        self.commit(&changes).await?;
        Ok(session.metadata(0, false))
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
    /// 副作用：更新标题与 `updated_at`，写入 `session_renamed` 事实事件。
    pub async fn rename_session(
        &self,
        id: &SessionId,
        title: &SessionTitle,
    ) -> Result<SessionMetadata, StoreError> {
        let _line = self.line.lock().await;
        let mut session = self.find_session(id).await?;
        session.title = title.clone();
        session.title_auto = false;
        session.updated_at = now_millis();
        let pending = self.storage.queue_length(id).await?;
        let busy = self.storage.busy(id).await?;
        let mut changes = Changeset::new();
        changes.update_session(
            session.id.clone(),
            session.title.clone(),
            session.title_auto,
            session.updated_at,
            session.active_leaf,
        );
        changes.append_event(
            FactEvent::SessionRenamed {
                session: session.metadata(pending, busy),
            },
            session.updated_at,
        );
        self.commit(&changes).await?;
        Ok(session.metadata(pending, busy))
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
        let _line = self.line.lock().await;
        self.find_session(id).await?;
        let mut changes = Changeset::new();
        changes.delete_session(id.clone());
        changes.append_event(
            FactEvent::SessionDeleted {
                session_id: id.clone(),
            },
            now_millis(),
        );
        self.commit(&changes).await.map(|_| ())
    }

    /// 读取单个 session 的消息树。
    ///
    /// # Errors
    ///
    /// - `StoreError::SessionNotFound`：session 不存在。
    /// - `StoreError::Database`：数据库读取失败。
    /// - `StoreError::CorruptedData`：存储值不符合领域约束。
    pub async fn session_tree(&self, id: &SessionId) -> Result<SessionTree, StoreError> {
        self.storage
            .session_tree(id)
            .await?
            .map(snapshot_into_tree)
            .ok_or_else(|| StoreError::SessionNotFound(id.clone()))
    }

    /// 读取 session 的 transcript：root 到 active leaf 的路径。
    ///
    /// # Errors
    ///
    /// - `StoreError::SessionNotFound`：session 不存在。
    /// - `StoreError::Database`：数据库读取失败。
    /// - `StoreError::CorruptedData`：存储值不符合领域约束。
    pub async fn transcript(&self, id: &SessionId) -> Result<Vec<MessageNode>, StoreError> {
        let tree = self.session_tree(id).await?;
        Ok(path_to_leaf(&tree.nodes, tree.active_leaf))
    }

    /// 分支命令的前置条件：session 空闲且队列为空。队列非空优先于 run 进行中报告。
    pub(super) async fn ensure_idle(&self, id: &SessionId) -> Result<(), StoreError> {
        if self.storage.queue_length(id).await? > 0 {
            return Err(StoreError::QueueNotEmpty(id.clone()));
        }
        if self.storage.busy(id).await? {
            return Err(StoreError::RunInProgress(id.clone()));
        }
        Ok(())
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

    /// session 是否有 running 的 run。
    ///
    /// # Errors
    ///
    /// - `StoreError::Database`：数据库读取失败。
    pub async fn busy(&self, id: &SessionId) -> Result<bool, StoreError> {
        self.storage.busy(id).await
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
