//! 队列写入：用户消息入队、插话提交、步骤边界接入与 run 结束降级。
//! 取出队首并建 run 在 `run`；自动标题规则在 `crate::session`；写命令入口在 `command`。

use super::{Store, append_node};
use std::collections::HashSet;

use crate::error::StoreError;
use crate::event::FactEvent;
use crate::id::{MessageId, QueueItemId, RunId, SessionId};
use crate::message::{ContentPart, MessageNode, MessageRole, MessageSkeleton, MessageText};
use crate::queue::{QueueItem, SubmitMode};
use crate::session::derive_title;
use crate::store::SessionCommand;

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
    /// 副作用：写入队列项；插话另写入用户节点与 active leaf。
    /// 必要时把自动标题改为由首行推导的标题。两者都写入 `message_queued` 事实事件。
    pub async fn enqueue_message(
        &self,
        id: &SessionId,
        text: MessageText,
        mode: SubmitMode,
    ) -> Result<QueueItem, StoreError> {
        self.command(id, async |command| {
            let now = command.now();
            if command.session().title_auto
                && command.queue_length() == 0
                && self.storage.message_count(id).await? == 0
            {
                command.session_mut().title = derive_title(&text)
                    .map_err(|error| StoreError::Invariant(format!("自动标题推导失败: {error}")))?;
            }
            match mode {
                SubmitMode::Steer if command.busy() => self.submit_steer(id, text, command).await,
                _ => {
                    let item = QueueItem {
                        id: self.storage.mint_queue_item_id().await?,
                        text,
                        anchor: None,
                        created_at: now,
                        pending_steer: false,
                    };
                    command
                        .changes()
                        .insert_queue_item(id.clone(), item.clone());
                    command.changes().append_event(
                        FactEvent::MessageQueued {
                            session_id: id.clone(),
                            item: item.clone(),
                        },
                        now,
                    );
                    Ok(item)
                }
            }
        })
        .await
    }

    /// 把插话写成 active leaf 下的用户节点与待插入队列项。
    async fn submit_steer(
        &self,
        id: &SessionId,
        text: MessageText,
        command: &mut SessionCommand<'_>,
    ) -> Result<QueueItem, StoreError> {
        let now = command.now();
        let node = MessageNode {
            id: self.storage.mint_message_id().await?,
            parent_id: command.session().active_leaf,
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
            pending_steer: true,
        };
        append_node(command, &node);
        command
            .changes()
            .insert_queue_item(id.clone(), item.clone());
        command.changes().append_event(
            FactEvent::MessageQueued {
                session_id: id.clone(),
                item: item.clone(),
            },
            now,
        );
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
        self.command(id, async |command| {
            let items = self.storage.pending_steer_items(id).await?;
            if items.is_empty() {
                return Ok(Vec::new());
            }
            let layout = self
                .layout_pending_steers(id, command.session().active_leaf, &items, Some(run_id))
                .await?;
            let now = command.now();
            rebind_changed(command, id, &layout, now);
            let mut applied = Vec::new();
            for (item, node) in items.iter().zip(&layout.steers) {
                command.changes().bind_message_run(node.id, run_id);
                command.changes().delete_queue_item(id.clone(), item.id);
                command.changes().append_event(
                    FactEvent::QueueItemRemoved {
                        session_id: id.clone(),
                        item_id: item.id,
                    },
                    now,
                );
                command.changes().append_event(
                    FactEvent::SteerApplied {
                        session_id: id.clone(),
                        run_id,
                        item_id: item.id,
                        message_id: node.id,
                    },
                    now,
                );
                applied.push(node.id);
            }
            move_leaf_to_last_steer(command, &layout);
            Ok(applied)
        })
        .await
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
        self.command(id, async |command| {
            let items = self.storage.pending_steer_items(id).await?;
            if items.is_empty() {
                return Ok(Vec::new());
            }
            let layout = self
                .layout_pending_steers(id, command.session().active_leaf, &items, None)
                .await?;
            let now = command.now();
            rebind_changed(command, id, &layout, now);
            let mut downgraded = Vec::new();
            for (item, _node) in items.iter().zip(&layout.steers) {
                command.changes().clear_pending_steer(id.clone(), item.id);
                let item = QueueItem {
                    pending_steer: false,
                    ..item.clone()
                };
                command.changes().append_event(
                    FactEvent::SteerDowngraded {
                        session_id: id.clone(),
                        item: item.clone(),
                    },
                    now,
                );
                downgraded.push(item);
            }
            move_leaf_to_last_steer(command, &layout);
            Ok(downgraded)
        })
        .await
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
        self.command(id, async |command| {
            if self.storage.queue_item(id, item_id).await?.is_none() {
                return Err(StoreError::QueueItemNotFound(id.clone(), item_id));
            }
            let now = command.now();
            command.changes().delete_queue_item(id.clone(), item_id);
            command.changes().append_event(
                FactEvent::QueueItemRemoved {
                    session_id: id.clone(),
                    item_id,
                },
                now,
            );
            Ok(())
        })
        .await
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
        leaf: Option<MessageId>,
        items: &[QueueItem],
        run_id: Option<RunId>,
    ) -> Result<SteerLayout, StoreError> {
        let tree = self.message_tree(id).await?;
        let chain = leaf.map(|leaf| tree.transcript(leaf)).unwrap_or_default();
        let mut stored = Vec::new();
        for item in items {
            stored.push(self.pending_steer_node(id, item).await?.skeleton());
        }
        let pending: Vec<MessageId> = stored.iter().map(|node| node.id).collect();
        let on_chain: HashSet<MessageId> = chain.iter().map(|node| node.id).collect();
        let mut changed = Vec::new();
        for node in &stored {
            let Some(child) = tree
                .children_of(node.id)
                .find(|child| on_chain.contains(&child.id))
            else {
                continue;
            };
            if pending.contains(&child.id) {
                continue;
            }
            changed.push(MessageSkeleton {
                parent_id: tree.ancestor(node.id, |candidate| !pending.contains(&candidate.id)),
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
            let steered = MessageSkeleton {
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
    changed: Vec<MessageSkeleton>,
    /// 按提交顺序排列的插话节点。
    steers: Vec<MessageSkeleton>,
}

/// 登记插话重排：被拆出的子节点改写父节点，并写 `message_updated`。
fn rebind_changed(
    command: &mut SessionCommand<'_>,
    session: &SessionId,
    layout: &SteerLayout,
    now: i64,
) {
    for node in &layout.changed {
        command
            .changes()
            .rebind_message_parent(node.id, node.parent_id);
        command.changes().append_event(
            FactEvent::MessageUpdated {
                session_id: session.clone(),
                message: node.clone(),
            },
            now,
        );
    }
}

/// 把 active leaf 移到最后一个插话。叶子没变时什么都不写。
fn move_leaf_to_last_steer(command: &mut SessionCommand<'_>, layout: &SteerLayout) {
    let Some(leaf) = layout.steers.last().map(|node| node.id) else {
        return;
    };
    if Some(leaf) == command.session().active_leaf {
        return;
    }
    let session_id = command.session().id.clone();
    let now = command.now();
    command.session_mut().active_leaf = Some(leaf);
    command.changes().append_event(
        FactEvent::ActiveLeafChanged {
            session_id,
            active_leaf: Some(leaf),
        },
        now,
    );
}
