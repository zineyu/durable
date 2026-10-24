//! 分支命令：编辑用户消息、重试 assistant、切换 active leaf。
//! 三者都要求 session 空闲，判定在 `session`；消息树结构在 `crate::message`。

use super::{Store, branch_leaf, find_node, now_millis, user_ancestor};
use crate::change::Changeset;
use crate::error::StoreError;
use crate::event::FactEvent;
use crate::id::{MessageId, SessionId};
use crate::message::{ContentPart, Message, MessageNode, MessageRole, MessageText};
use crate::queue::QueueItem;

impl Store {
    /// 编辑一条历史用户消息：在其父节点下创建兄弟用户节点，把 active leaf 移到新节点并入队执行。
    ///
    /// 输入：session 身份、目标用户消息身份与已校验的新正文。
    ///
    /// # Errors
    ///
    /// - `StoreError::SessionNotFound`：session 不存在。
    /// - `StoreError::MessageNotFound`：目标不是该 session 的消息。
    /// - `StoreError::MessageRoleMismatch`：目标不是用户消息。
    /// - `StoreError::QueueNotEmpty`：session 队列非空。
    /// - `StoreError::RunInProgress`：session 有 run 进行中。
    /// - `StoreError::Database`：数据库读取或写入失败。
    /// - `StoreError::CorruptedData`：存储值不符合领域约束。
    /// - `StoreError::Serialization`：事实事件序列化失败。
    ///
    /// 副作用：写入新用户节点、锚定该节点的队列项与 active leaf，并写入事实事件。
    pub async fn edit_user_message(
        &self,
        id: &SessionId,
        target: MessageId,
        text: MessageText,
    ) -> Result<MessageNode, StoreError> {
        let _line = self.line.lock().await;
        let mut session = self.find_session(id).await?;
        self.ensure_idle(id).await?;
        let snapshot = self.session_snapshot(id).await?;
        let target_node = find_node(&snapshot.messages, target)
            .ok_or_else(|| StoreError::MessageNotFound(id.clone(), target))?;
        if target_node.role != MessageRole::User {
            return Err(StoreError::MessageRoleMismatch(target, target_node.role));
        }
        let created_at = now_millis();
        let node = MessageNode {
            id: self.storage.mint_message_id().await?,
            parent_id: target_node.parent_id,
            role: MessageRole::User,
            parts: vec![ContentPart::text(text.as_str())],
            run_id: None,
            created_at,
        };
        let item = QueueItem {
            id: self.storage.mint_queue_item_id().await?,
            text,
            anchor: Some(node.id),
            created_at,
            pending_steer: false,
        };
        session.updated_at = created_at;
        let metadata = session.metadata(1, false);
        let mut changes = Changeset::new();
        changes.insert_message(Message {
            session: id.clone(),
            node: node.clone(),
        });
        changes.insert_queue_item(id.clone(), item.clone());
        changes.update_session(
            id.clone(),
            session.title.clone(),
            session.title_auto,
            created_at,
            Some(node.id),
        );
        changes.append_event(
            FactEvent::MessageAppended {
                session: metadata.clone(),
                message: node.skeleton(),
            },
            created_at,
        );
        changes.append_event(
            FactEvent::ActiveLeafChanged {
                session: metadata.clone(),
                active_leaf: Some(node.id),
            },
            created_at,
        );
        changes.append_event(
            FactEvent::MessageQueued {
                session: metadata,
                item,
            },
            created_at,
        );
        self.commit(&changes).await?;
        Ok(node)
    }

    /// 重试一条 assistant 消息：以其父用户节点为锚入队，run 开始时创建同父兄弟 assistant 节点。
    ///
    /// 输入：session 身份与目标 assistant 消息身份。
    ///
    /// # Errors
    ///
    /// - `StoreError::SessionNotFound`：session 不存在。
    /// - `StoreError::MessageNotFound`：目标或目标的父节点不是该 session 的消息。
    /// - `StoreError::MessageRoleMismatch`：目标不是 assistant 消息。
    /// - `StoreError::Invariant`：目标 assistant 消息缺少父节点，或父链上没有用户消息。
    /// - `StoreError::QueueNotEmpty`：session 队列非空。
    /// - `StoreError::RunInProgress`：session 有 run 进行中。
    /// - `StoreError::CorruptedData`：用户消息正文不符合领域约束。
    /// - `StoreError::Database`：数据库读取或写入失败。
    /// - `StoreError::Serialization`：事实事件序列化失败。
    ///
    /// 副作用：写入锚定父用户节点的队列项与更新时间；不创建用户节点。
    pub async fn retry_assistant_message(
        &self,
        id: &SessionId,
        target: MessageId,
    ) -> Result<(), StoreError> {
        let _line = self.line.lock().await;
        let mut session = self.find_session(id).await?;
        self.ensure_idle(id).await?;
        let snapshot = self.session_snapshot(id).await?;
        let target_node = find_node(&snapshot.messages, target)
            .ok_or_else(|| StoreError::MessageNotFound(id.clone(), target))?;
        if target_node.role != MessageRole::Assistant {
            return Err(StoreError::MessageRoleMismatch(target, target_node.role));
        }
        let parent_id = target_node.parent_id.ok_or_else(|| {
            StoreError::Invariant(format!("assistant 消息 {} 缺少父节点", target.get()))
        })?;
        let user_id = user_ancestor(&snapshot.messages, parent_id).ok_or_else(|| {
            StoreError::Invariant(format!("assistant 消息 {} 之前没有用户消息", target.get()))
        })?;
        let stored = self
            .storage
            .message(id, user_id)
            .await?
            .ok_or_else(|| StoreError::MessageNotFound(id.clone(), user_id))?;
        let text = MessageText::parse(&stored.node.text())
            .map_err(|error| StoreError::CorruptedData(format!("message.parts: {error}")))?;
        let created_at = now_millis();
        let item = QueueItem {
            id: self.storage.mint_queue_item_id().await?,
            text,
            anchor: Some(parent_id),
            created_at,
            pending_steer: false,
        };
        session.updated_at = created_at;
        let metadata = session.metadata(1, false);
        let mut changes = Changeset::new();
        changes.insert_queue_item(id.clone(), item.clone());
        changes.update_session(
            id.clone(),
            session.title.clone(),
            session.title_auto,
            created_at,
            session.active_leaf,
        );
        changes.append_event(
            FactEvent::MessageQueued {
                session: metadata,
                item,
            },
            created_at,
        );
        self.commit(&changes).await?;
        Ok(())
    }

    /// 切换 active leaf：把 active leaf 移到目标子树中最近创建的叶子。
    ///
    /// 输入：session 身份与目标消息身份。
    ///
    /// # Errors
    ///
    /// - `StoreError::SessionNotFound`：session 不存在。
    /// - `StoreError::MessageNotFound`：目标不属于该 session。
    /// - `StoreError::Invariant`：目标子树没有叶子。
    /// - `StoreError::QueueNotEmpty`：session 队列非空。
    /// - `StoreError::RunInProgress`：session 有 run 进行中。
    /// - `StoreError::Database`：数据库读取或写入失败。
    /// - `StoreError::CorruptedData`：存储值不符合领域约束。
    /// - `StoreError::Serialization`：事实事件序列化失败。
    ///
    /// 副作用：更新 active leaf 与更新时间，写入 `active_leaf_changed` 事实事件。
    pub async fn switch_active_leaf(
        &self,
        id: &SessionId,
        target: MessageId,
    ) -> Result<MessageId, StoreError> {
        let _line = self.line.lock().await;
        let mut session = self.find_session(id).await?;
        self.ensure_idle(id).await?;
        let snapshot = self.session_snapshot(id).await?;
        let nodes = snapshot.messages.clone();
        if !nodes.iter().any(|node| node.id == target) {
            return Err(StoreError::MessageNotFound(id.clone(), target));
        }
        let leaf = branch_leaf(&nodes, target).ok_or_else(|| {
            StoreError::Invariant(format!("目标 {} 的子树没有叶子", target.get()))
        })?;
        let now = now_millis();
        session.updated_at = now;
        let metadata = session.metadata(snapshot.queue_length, snapshot.busy);
        let mut changes = Changeset::new();
        changes.update_session(
            id.clone(),
            session.title.clone(),
            session.title_auto,
            now,
            Some(leaf),
        );
        changes.append_event(
            FactEvent::ActiveLeafChanged {
                session: metadata,
                active_leaf: Some(leaf),
            },
            now,
        );
        self.commit(&changes).await?;
        Ok(leaf)
    }
}
