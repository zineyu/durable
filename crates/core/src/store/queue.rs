//! 队列写入：用户消息入队、插话提交、步骤边界接入与 run 结束降级。
//! 取出队首并建 run 在 `run`；自动标题规则在 `crate::session`。

use super::{Store, now_millis};
use crate::change::Changeset;
use crate::error::StoreError;
use crate::event::FactEvent;
use crate::id::{MessageId, QueueItemId, RunId, SessionId};
use crate::message::{ContentPart, Message, MessageNode, MessageRole, MessageText};
use crate::queue::{QueueItem, SubmitMode};
use crate::session::{SessionMetadata, SessionRecord, derive_title};

impl Store {
    /// 提交一条用户消息。`mode` 决定它是排队还是插话。
    ///
    /// 排队语义不创建消息节点，不移动 active leaf。插话语义要求该 session 有活动 run：
    /// 消息立即成为 active leaf 下的用户节点，队列项标记为待插入，等待步骤边界接入。
    /// 没有活动 run 时插话退化为排队。
    ///
    /// # Errors
    ///
    /// - `StoreError::SessionNotFound`：session 不存在。
    /// - `StoreError::Invariant`：无法由正文推导出合法标题。
    /// - `StoreError::Database`：数据库读取或写入失败。
    /// - `StoreError::CorruptedData`：存储值不符合领域约束。
    /// - `StoreError::Serialization`：事实事件序列化失败。
    ///
    /// 副作用：写入队列项；插话另写入用户节点、active leaf 与更新时间。
    /// 必要时把自动标题改为由首行推导的标题。两者都写入 `message_queued` 事实事件。
    pub async fn enqueue_message(
        &self,
        id: &SessionId,
        text: MessageText,
        mode: SubmitMode,
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
        let metadata = session.metadata(pending + 1, busy);
        let mut changes = Changeset::new();
        let item = match mode {
            SubmitMode::Steer if busy => {
                self.submit_steer(id, text, &mut session, metadata, created_at, &mut changes)
                    .await?
            }
            _ => {
                let item = QueueItem {
                    id: self.storage.mint_queue_item_id().await?,
                    text,
                    anchor: None,
                    created_at,
                    pending_steer: false,
                };
                changes.insert_queue_item(id.clone(), item.clone());
                changes.append_event(
                    FactEvent::MessageQueued {
                        session: metadata,
                        item: item.clone(),
                    },
                    created_at,
                );
                item
            }
        };
        changes.update_session(
            id.clone(),
            session.title.clone(),
            session.title_auto,
            created_at,
            session.active_leaf,
        );
        self.commit(&changes).await?;
        Ok(item)
    }

    /// 把插话写成 active leaf 下的用户节点与待插入队列项。
    async fn submit_steer(
        &self,
        id: &SessionId,
        text: MessageText,
        session: &mut SessionRecord,
        metadata: SessionMetadata,
        created_at: i64,
        changes: &mut Changeset,
    ) -> Result<QueueItem, StoreError> {
        let node = MessageNode {
            id: self.storage.mint_message_id().await?,
            parent_id: session.active_leaf,
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
            pending_steer: true,
        };
        changes.insert_message(Message {
            session: id.clone(),
            node: node.clone(),
        });
        changes.insert_queue_item(id.clone(), item.clone());
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
                item: item.clone(),
            },
            created_at,
        );
        session.active_leaf = Some(node.id);
        Ok(item)
    }

    /// 步骤边界：把待插入插话按提交顺序接入链尾，并绑定当前 run。
    ///
    /// 接入即消费，队列项被删除。节点的父节点改为当时的 active leaf，因此提交期间已定型的
    /// assistant 消息留在插话之前：模型看到的顺序是「旧回答，然后是修正」。
    ///
    /// # Errors
    ///
    /// - `StoreError::SessionNotFound`：session 不存在。
    /// - `StoreError::MessageNotFound`：插话锚定的消息不存在。
    /// - `StoreError::Invariant`：待插入队列项缺少锚点，或项数超出 `u32`。
    /// - `StoreError::Database`：数据库读取或写入失败。
    /// - `StoreError::Serialization`：事实事件序列化失败。
    ///
    /// 副作用：改写插话节点的父节点与 run 关联，删除队列项，移动 active leaf，
    /// 写入 `message_updated`、`queue_item_removed`、`steer_applied` 与 `active_leaf_changed`。
    pub async fn apply_pending_steers(
        &self,
        id: &SessionId,
        run_id: RunId,
    ) -> Result<Vec<MessageId>, StoreError> {
        let _line = self.line.lock().await;
        let mut session = self.find_session(id).await?;
        let items = self.storage.pending_steer_items(id).await?;
        if items.is_empty() {
            return Ok(Vec::new());
        }
        let pending = self.storage.queue_length(id).await?;
        let consumed = u32::try_from(items.len())
            .map_err(|_| StoreError::Invariant(format!("待插入项数超出 u32: {}", items.len())))?;
        let metadata = session.metadata(pending.saturating_sub(consumed), true);
        let layout = self.layout_pending_steers(id, &items, Some(run_id)).await?;
        let now = now_millis();
        let mut changes = Changeset::new();
        for node in &layout.changed {
            changes.rebind_message_parent(node.id, node.parent_id);
            changes.append_event(
                FactEvent::MessageUpdated {
                    session: metadata.clone(),
                    message: node.skeleton(),
                },
                now,
            );
        }
        let mut applied = Vec::new();
        for (item, node) in items.iter().zip(&layout.steers) {
            changes.bind_message_run(node.id, run_id);
            changes.delete_queue_item(id.clone(), item.id);
            changes.append_event(
                FactEvent::QueueItemRemoved {
                    session_id: id.clone(),
                    item_id: item.id,
                },
                now,
            );
            changes.append_event(
                FactEvent::SteerApplied {
                    session: metadata.clone(),
                    run_id,
                    item_id: item.id,
                    message_id: node.id,
                },
                now,
            );
            applied.push(node.id);
        }
        move_leaf_to_last_steer(&mut changes, &mut session, &layout, metadata, now);
        self.commit(&changes).await?;
        Ok(applied)
    }

    /// run 结束时仍未消费的插话降级为普通排队项。
    ///
    /// 节点留在链尾，下一次 run 以它为起点接续；队列项保留下来作为启动信号。
    ///
    /// # Errors
    ///
    /// 同 [`Store::apply_pending_steers`]。
    ///
    /// 副作用：清除待插入标记、必要时改写节点父节点与 active leaf，
    /// 写入 `message_updated`、`steer_downgraded` 与 `active_leaf_changed`。
    pub async fn downgrade_pending_steers(
        &self,
        id: &SessionId,
    ) -> Result<Vec<QueueItem>, StoreError> {
        let _line = self.line.lock().await;
        let mut session = self.find_session(id).await?;
        let items = self.storage.pending_steer_items(id).await?;
        if items.is_empty() {
            return Ok(Vec::new());
        }
        let pending = self.storage.queue_length(id).await?;
        let busy = self.storage.busy(id).await?;
        let metadata = session.metadata(pending, busy);
        let layout = self.layout_pending_steers(id, &items, None).await?;
        let now = now_millis();
        let mut changes = Changeset::new();
        for node in &layout.changed {
            changes.rebind_message_parent(node.id, node.parent_id);
            changes.append_event(
                FactEvent::MessageUpdated {
                    session: metadata.clone(),
                    message: node.skeleton(),
                },
                now,
            );
        }
        let mut downgraded = Vec::new();
        for (item, _node) in items.iter().zip(&layout.steers) {
            changes.clear_pending_steer(id.clone(), item.id);
            let item = QueueItem {
                pending_steer: false,
                ..item.clone()
            };
            changes.append_event(
                FactEvent::SteerDowngraded {
                    session: metadata.clone(),
                    item: item.clone(),
                },
                now,
            );
            downgraded.push(item);
        }
        move_leaf_to_last_steer(&mut changes, &mut session, &layout, metadata, now);
        self.commit(&changes).await?;
        Ok(downgraded)
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
    /// 插话项被删除后其节点留在树上，成为没有 run 的分支。
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

    /// 计算插话在链上的新位置。
    ///
    /// 插话在提交时就已经是链上的节点，提交之后定型的消息挂在它们下面。重排分两步：
    /// 先把每个插话从链上摘下（它的链上子节点改挂到最近的非插话祖先），再按提交顺序接到链尾。
    /// 拆出写在接续之前，客户端逐条应用事件时才不会看到环。
    ///
    /// `run_id` 为 `None` 时不改 run 关联，用于降级。
    async fn layout_pending_steers(
        &self,
        id: &SessionId,
        items: &[QueueItem],
        run_id: Option<RunId>,
    ) -> Result<SteerLayout, StoreError> {
        let chain = self.storage.transcript(id).await?;
        let mut stored = Vec::new();
        for item in items {
            stored.push(self.pending_steer_node(id, item).await?);
        }
        let pending: Vec<MessageId> = stored.iter().map(|node| node.id).collect();
        let mut changed = Vec::new();
        for node in &stored {
            let Some(child) = chain.iter().find(|entry| entry.parent_id == Some(node.id)) else {
                continue;
            };
            if pending.contains(&child.id) {
                continue;
            }
            changed.push(MessageNode {
                parent_id: ancestor(&chain, &pending, node.parent_id),
                ..child.clone()
            });
        }
        let mut previous = chain
            .iter()
            .rev()
            .find(|node| !pending.contains(&node.id))
            .map(|node| node.id);
        let mut steers = Vec::new();
        for node in &stored {
            let steered = MessageNode {
                parent_id: previous,
                run_id: run_id.or(node.run_id),
                ..node.clone()
            };
            previous = Some(steered.id);
            changed.push(steered.clone());
            steers.push(steered);
        }
        Ok(SteerLayout { changed, steers })
    }

    /// 待插入队列项锚定的消息节点。
    async fn pending_steer_node(
        &self,
        id: &SessionId,
        item: &QueueItem,
    ) -> Result<MessageNode, StoreError> {
        let anchor = item.anchor.ok_or_else(|| {
            StoreError::Invariant(format!("待插入队列项 {} 缺少锚点", item.id.get()))
        })?;
        let stored = self
            .storage
            .message(id, anchor)
            .await?
            .ok_or_else(|| StoreError::MessageNotFound(id.clone(), anchor))?;
        Ok(stored.node)
    }
}

/// 插话重排的结果。
struct SteerLayout {
    /// 需要改写父节点并写 `message_updated` 的节点：拆出的子节点在前，插话按提交顺序在后。
    changed: Vec<MessageNode>,
    /// 按提交顺序排列的插话节点。
    steers: Vec<MessageNode>,
}

/// 沿父链找到最近的、不属于插话的祖先。
fn ancestor(
    chain: &[MessageNode],
    pending: &[MessageId],
    start: Option<MessageId>,
) -> Option<MessageId> {
    let mut current = start;
    while let Some(id) = current {
        if !pending.contains(&id) {
            return Some(id);
        }
        current = chain
            .iter()
            .find(|node| node.id == id)
            .and_then(|node| node.parent_id);
    }
    None
}

/// 把 active leaf 移到最后一个插话。叶子没变时什么都不写。
fn move_leaf_to_last_steer(
    changes: &mut Changeset,
    session: &mut SessionRecord,
    layout: &SteerLayout,
    metadata: SessionMetadata,
    now: i64,
) {
    let Some(leaf) = layout.steers.last().map(|node| node.id) else {
        return;
    };
    if Some(leaf) == session.active_leaf {
        return;
    }
    session.updated_at = now;
    changes.update_session(
        session.id.clone(),
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
}
