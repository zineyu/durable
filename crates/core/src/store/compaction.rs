//! 压缩消息写入：摘要挂在写入时的 active leaf 下，不属于任何 run。
//! run 生命周期在 `run`，session 元数据在 `session`，transcript 回溯在 `crate::storage`。

use super::{Store, now_millis};
use crate::change::Changeset;
use crate::error::StoreError;
use crate::event::FactEvent;
use crate::id::{MessageId, SessionId};
use crate::message::{ContentPart, Message, MessageNode, MessageRole};

impl Store {
    /// 把一条压缩消息入树：`role` 为压缩、`run_id` 为 `None`，父节点是当时的 active leaf。
    ///
    /// 输入：session 身份、已铸造的消息 id 与摘要的内容块。
    ///
    /// # Errors
    ///
    /// - `StoreError::SessionNotFound`：session 不存在。
    /// - `StoreError::Database`：数据库读取或写入失败。
    /// - `StoreError::Serialization`：事实事件序列化失败。
    ///
    /// 副作用：写入压缩节点、active leaf 与 `message_appended`、`active_leaf_changed` 事实事件。
    pub async fn append_compaction(
        &self,
        session: &SessionId,
        id: MessageId,
        parts: Vec<ContentPart>,
    ) -> Result<MessageNode, StoreError> {
        let _line = self.line.lock().await;
        let mut record = self.find_session(session).await?;
        let now = now_millis();
        let pending = self.storage.queue_length(session).await?;
        let metadata = record.metadata(pending, true);
        let node = MessageNode {
            id,
            parent_id: record.active_leaf,
            role: MessageRole::Compaction,
            parts,
            run_id: None,
            created_at: now,
        };
        record.updated_at = now;
        let mut changes = Changeset::new();
        changes.insert_message(Message {
            session: session.clone(),
            node: node.clone(),
        });
        changes.update_session(
            session.clone(),
            record.title.clone(),
            record.title_auto,
            now,
            Some(node.id),
        );
        changes.append_event(
            FactEvent::MessageAppended {
                session: metadata.clone(),
                message: node.skeleton(),
            },
            now,
        );
        changes.append_event(
            FactEvent::ActiveLeafChanged {
                session: metadata,
                active_leaf: Some(node.id),
            },
            now,
        );
        self.commit(&changes).await?;
        Ok(node)
    }
}
