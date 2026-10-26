//! 分支命令：编辑用户消息、重试 assistant、切换 active leaf。
//! 三者都要求 session 空闲，判定由 `command_idle` 完成；树的问题在 `crate::tree`。

use super::{Store, append_node};
use crate::error::StoreError;
use crate::event::FactEvent;
use crate::id::{MessageId, SessionId};
use crate::message::{ContentPart, MessageNode, MessageRole, MessageSkeleton, MessageText};
use crate::queue::QueueItem;
use crate::tree::MessageTree;

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
        self.command_idle(id, async |command| {
            let snapshot = self.session_snapshot(id).await?;
            let tree = MessageTree::new(snapshot.messages);
            let target_node = stored_node(&tree, id, target)?;
            if target_node.role != MessageRole::User {
                return Err(StoreError::MessageRoleMismatch(target, target_node.role));
            }
            let now = command.now();
            let node = MessageNode {
                id: self.storage.mint_message_id().await?,
                parent_id: target_node.parent_id,
                role: MessageRole::User,
                parts: vec![ContentPart::text(text.as_str())],
                run_id: None,
                created_at: now,
            };
            let item = QueueItem {
                id: self.storage.mint_queue_item_id().await?,
                text,
                anchor: Some(node.id),
                created_at: now,
                pending_steer: false,
            };
            append_node(command, &node);
            command
                .changes()
                .insert_queue_item(id.clone(), item.clone());
            command.changes().append_event(
                FactEvent::MessageQueued {
                    session_id: id.clone(),
                    item,
                },
                now,
            );
            Ok(node)
        })
        .await
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
        self.command_idle(id, async |command| {
            let snapshot = self.session_snapshot(id).await?;
            let tree = MessageTree::new(snapshot.messages);
            let target_node = stored_node(&tree, id, target)?;
            if target_node.role != MessageRole::Assistant {
                return Err(StoreError::MessageRoleMismatch(target, target_node.role));
            }
            let parent_id = target_node.parent_id.ok_or_else(|| {
                StoreError::Invariant(format!("assistant 消息 {} 缺少父节点", target.get()))
            })?;
            let user_id = tree
                .ancestor(target, |node| node.role == MessageRole::User)
                .ok_or_else(|| {
                    StoreError::Invariant(format!(
                        "assistant 消息 {} 之前没有用户消息",
                        target.get()
                    ))
                })?;
            let stored = self
                .storage
                .message(id, user_id)
                .await?
                .ok_or_else(|| StoreError::MessageNotFound(id.clone(), user_id))?;
            let text = MessageText::parse(&stored.node.text())
                .map_err(|error| StoreError::CorruptedData(format!("message.parts: {error}")))?;
            let now = command.now();
            let item = QueueItem {
                id: self.storage.mint_queue_item_id().await?,
                text,
                anchor: Some(parent_id),
                created_at: now,
                pending_steer: false,
            };
            command
                .changes()
                .insert_queue_item(id.clone(), item.clone());
            command.changes().append_event(
                FactEvent::MessageQueued {
                    session_id: id.clone(),
                    item,
                },
                now,
            );
            Ok(())
        })
        .await
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
        self.command_idle(id, async |command| {
            let snapshot = self.session_snapshot(id).await?;
            let tree = MessageTree::new(snapshot.messages);
            if tree.get(target).is_none() {
                return Err(StoreError::MessageNotFound(id.clone(), target));
            }
            let leaf = tree.leaf_of(target).ok_or_else(|| {
                StoreError::Invariant(format!("目标 {} 的子树没有叶子", target.get()))
            })?;
            let now = command.now();
            command.session_mut().active_leaf = Some(leaf);
            command.changes().append_event(
                FactEvent::ActiveLeafChanged {
                    session_id: id.clone(),
                    active_leaf: Some(leaf),
                },
                now,
            );
            Ok(leaf)
        })
        .await
    }
}

/// 按 id 取树中的节点，不存在时为 `MessageNotFound`。
fn stored_node(
    tree: &MessageTree,
    session: &SessionId,
    target: MessageId,
) -> Result<MessageSkeleton, StoreError> {
    tree.get(target)
        .cloned()
        .ok_or_else(|| StoreError::MessageNotFound(session.clone(), target))
}
