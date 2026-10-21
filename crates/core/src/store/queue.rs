//! 队列写入：用户消息入队与未开始项的删除。
//! 取出队首并建 run 在 `run`；自动标题规则在 `crate::session`。

use super::{Store, now_millis};
use crate::change::Changeset;
use crate::error::StoreError;
use crate::event::FactEvent;
use crate::id::{QueueItemId, SessionId};
use crate::message::MessageText;
use crate::queue::QueueItem;
use crate::session::derive_title;

impl Store {
    /// 把一条用户消息写入 session 的 FIFO 队列。此阶段不创建消息节点，不移动 active leaf。
    ///
    /// 输入：session 身份与已校验的消息正文。
    ///
    /// # Errors
    ///
    /// - `StoreError::SessionNotFound`：session 不存在。
    /// - `StoreError::Invariant`：无法由正文推导出合法标题。
    /// - `StoreError::Database`：数据库读取或写入失败。
    /// - `StoreError::CorruptedData`：存储值不符合领域约束。
    /// - `StoreError::Serialization`：事实事件序列化失败。
    ///
    /// 副作用：写入队列项、更新 `updated_at`，必要时把自动标题改为由首行推导的标题，
    /// 并写入 `message_queued` 事实事件。
    pub async fn enqueue_message(
        &self,
        id: &SessionId,
        text: MessageText,
    ) -> Result<QueueItem, StoreError> {
        let _line = self.line.lock().await;
        let mut session = self.find_session(id).await?;
        let pending = self.storage.queue_length(id).await?;
        let messages = self.storage.message_count(id).await?;
        let busy = self.storage.busy(id).await?;
        let created_at = now_millis();
        if session.title_auto && pending == 0 && messages == 0 {
            session.title = derive_title(&text)
                .map_err(|error| StoreError::Invariant(format!("自动标题推导失败: {error}")))?;
        }
        session.updated_at = created_at;
        let item = QueueItem {
            id: self.storage.mint_queue_item_id().await?,
            text,
            anchor: None,
            created_at,
        };
        let mut changes = Changeset::new();
        changes.insert_queue_item(id.clone(), item.clone());
        changes.update_session(
            session.id.clone(),
            session.title.clone(),
            session.title_auto,
            session.updated_at,
            session.active_leaf,
        );
        changes.append_event(
            FactEvent::MessageQueued {
                session: session.metadata(pending + 1, busy),
                item: item.clone(),
            },
            created_at,
        );
        self.commit(&changes).await?;
        Ok(item)
    }

    /// 删除尚未开始的队列项。
    ///
    /// 输入：session 身份与队列项身份。
    ///
    /// # Errors
    ///
    /// - `StoreError::SessionNotFound`：session 不存在。
    /// - `StoreError::QueueItemNotFound`：该项不在该 session 的队列中。
    /// - `StoreError::Database`：数据库读取或写入失败。
    /// - `StoreError::Serialization`：事实事件序列化失败。
    ///
    /// 副作用：删除队列项，写入 `queue_item_removed` 事实事件。
    pub async fn remove_queue_item(
        &self,
        id: &SessionId,
        item_id: QueueItemId,
    ) -> Result<(), StoreError> {
        let _line = self.line.lock().await;
        self.find_session(id).await?;
        if self.storage.queue_item(id, item_id).await?.is_none() {
            return Err(StoreError::QueueItemNotFound(id.clone(), item_id));
        }
        let mut changes = Changeset::new();
        changes.delete_queue_item(id.clone(), item_id);
        changes.append_event(
            FactEvent::QueueItemRemoved {
                session_id: id.clone(),
                item_id,
            },
            now_millis(),
        );
        self.commit(&changes).await.map(|_| ())
    }
}
