//! run 的生命周期写入：开始、流式落库、工具调用、结束与启动恢复。
//! 调度在 `crates/agent/src/engine`；队列项在此之前写入 `queue`。

use super::{Store, now_millis};
use crate::change::Changeset;
use crate::error::StoreError;
use crate::event::FactEvent;
use crate::id::{MessageId, SessionId};
use crate::message::{Message, MessageContent, MessageNode, MessageRole};
use crate::queue::QueueItem;
use crate::run::{RunFailure, RunFailureCode, RunRecord, RunStatus};
use crate::session::SessionMetadata;
use crate::store::RunStart;
use crate::tool::ToolCallRecord;

impl Store {
    /// 开始执行 session 队首的队列项：建 assistant 节点、建 running 的 run、移动 active leaf。
    ///
    /// 队列项未锚定用户节点时在当前 active leaf 下新建用户节点；已锚定时接续该节点，
    /// 已存在但未绑定 run 的节点补上 run 关联。
    ///
    /// # Errors
    ///
    /// - `StoreError::SessionNotFound`：session 不存在。
    /// - `StoreError::MessageNotFound`：锚定的消息不是该 session 的消息。
    /// - `StoreError::MessageRoleMismatch`：锚定的消息不是用户消息。
    /// - `StoreError::Database`：数据库读取或写入失败。
    /// - `StoreError::CorruptedData`：存储值不符合领域约束。
    /// - `StoreError::Serialization`：事实事件序列化失败。
    ///
    /// 副作用：删除队首项，写入消息节点、run 行、active leaf 与事实事件。
    /// 队列为空或该 session 已有 running 的 run 时返回 `None`。
    pub async fn start_run(&self, id: &SessionId) -> Result<Option<RunStart>, StoreError> {
        let _line = self.line.lock().await;
        let mut session = self.find_session(id).await?;
        if self.storage.busy(id).await? {
            return Ok(None);
        }
        let Some(item) = self.storage.first_queue_item(id).await? else {
            return Ok(None);
        };
        let now = now_millis();
        session.updated_at = now;
        let run_id = self.storage.mint_run_id().await?;
        let pending = self.storage.queue_length(id).await?;
        let remaining = pending.saturating_sub(1);
        let metadata = session.metadata(remaining, true);
        let run = RunRecord {
            id: run_id,
            session_id: id.clone(),
            status: RunStatus::Running,
            failure: None,
            created_at: now,
            updated_at: now,
        };
        let mut changes = Changeset::new();
        changes.delete_queue_item(id.clone(), item.id);
        changes.insert_run(run.clone());
        let user = self
            .run_user_node(&item, &run, session.active_leaf, &metadata, &mut changes)
            .await?;
        let assistant = self
            .run_assistant_node(&user, &run, &metadata, &mut changes)
            .await?;
        changes.update_session(
            id.clone(),
            session.title.clone(),
            session.title_auto,
            now,
            Some(assistant.id),
        );
        changes.append_event(
            FactEvent::ActiveLeafChanged {
                session: metadata.clone(),
                active_leaf: Some(assistant.id),
            },
            now,
        );
        changes.append_event(
            FactEvent::QueueItemRemoved {
                session_id: id.clone(),
                item_id: item.id,
            },
            now,
        );
        changes.append_event(
            FactEvent::RunStarted {
                session: metadata,
                run: run.clone(),
            },
            now,
        );
        self.commit(&changes).await?;
        Ok(Some(RunStart {
            run,
            workspace: session.workspace.clone(),
            user,
            assistant,
        }))
    }

    /// 登记 run 的用户节点：接续队首项锚定的用户节点，或新建挂在 `parent` 下的用户节点。
    async fn run_user_node(
        &self,
        item: &QueueItem,
        run: &RunRecord,
        parent: Option<MessageId>,
        metadata: &SessionMetadata,
        changes: &mut Changeset,
    ) -> Result<MessageNode, StoreError> {
        let session = &run.session_id;
        let Some(anchor) = item.anchor else {
            let user = MessageNode {
                id: self.storage.mint_message_id().await?,
                parent_id: parent,
                role: MessageRole::User,
                text: MessageContent::new(item.text.as_str()),
                run_id: Some(run.id),
                tool_calls: Vec::new(),
                created_at: run.created_at,
            };
            changes.insert_message(Message {
                session: session.clone(),
                node: user.clone(),
            });
            changes.append_event(
                FactEvent::MessageAppended {
                    session: metadata.clone(),
                    message: user.clone(),
                },
                run.created_at,
            );
            return Ok(user);
        };
        let message = self
            .storage
            .message(session, anchor)
            .await?
            .ok_or_else(|| StoreError::MessageNotFound(session.clone(), anchor))?;
        if message.node.role != MessageRole::User {
            return Err(StoreError::MessageRoleMismatch(anchor, message.node.role));
        }
        if message.node.run_id.is_some() {
            return Ok(message.node);
        }
        let bound = MessageNode {
            run_id: Some(run.id),
            ..message.node
        };
        changes.bind_message_run(bound.id, run.id);
        changes.append_event(
            FactEvent::MessageUpdated {
                session: metadata.clone(),
                message: bound.clone(),
            },
            run.created_at,
        );
        Ok(bound)
    }

    /// 登记 run 的 assistant 节点：挂在 `user` 之下，内容为空。
    async fn run_assistant_node(
        &self,
        user: &MessageNode,
        run: &RunRecord,
        metadata: &SessionMetadata,
        changes: &mut Changeset,
    ) -> Result<MessageNode, StoreError> {
        let assistant = MessageNode {
            id: self.storage.mint_message_id().await?,
            parent_id: Some(user.id),
            role: MessageRole::Assistant,
            text: MessageContent::empty(),
            run_id: Some(run.id),
            tool_calls: Vec::new(),
            created_at: run.created_at,
        };
        changes.insert_message(Message {
            session: run.session_id.clone(),
            node: assistant.clone(),
        });
        changes.append_event(
            FactEvent::MessageAppended {
                session: metadata.clone(),
                message: assistant.clone(),
            },
            run.created_at,
        );
        Ok(assistant)
    }

    /// 节流写入 assistant 节点的部分输出。此写入不产生事实事件。
    ///
    /// # Errors
    ///
    /// - `StoreError::Invariant`：该消息不存在，更新未命中任何行。
    /// - `StoreError::Database`：数据库写入失败。
    /// - `StoreError::Serialization`：工具调用序列化失败。
    pub async fn flush_message(
        &self,
        message_id: MessageId,
        text: MessageContent,
        tool_calls: Vec<ToolCallRecord>,
    ) -> Result<(), StoreError> {
        let _line = self.line.lock().await;
        let mut changes = Changeset::new();
        changes.update_message(message_id, text, tool_calls);
        self.storage.apply(&changes).await?;
        Ok(())
    }

    /// 记录一次已完成的工具调用：把记录追加到 assistant 节点，并写入事实事件。
    ///
    /// # Errors
    ///
    /// - `StoreError::SessionNotFound`：session 不存在。
    /// - `StoreError::MessageNotFound`：消息不存在。
    /// - `StoreError::Database`：数据库读取或写入失败。
    /// - `StoreError::CorruptedData`：存储值不符合领域约束。
    /// - `StoreError::Serialization`：事实事件序列化失败。
    ///
    /// 副作用：更新消息节点的工具调用；写入 `tool_call_recorded` 事件。
    pub async fn record_tool_call(
        &self,
        session: &SessionId,
        message_id: MessageId,
        call: ToolCallRecord,
    ) -> Result<(), StoreError> {
        let _line = self.line.lock().await;
        let record = self.find_session(session).await?;
        let pending = self.storage.queue_length(session).await?;
        let metadata = record.metadata(pending, true);
        let stored = self
            .storage
            .message(session, message_id)
            .await?
            .ok_or_else(|| StoreError::MessageNotFound(session.clone(), message_id))?;
        let Message { node, .. } = stored;
        let mut tool_calls = node.tool_calls;
        tool_calls.push(call.clone());
        let mut changes = Changeset::new();
        changes.update_message(message_id, node.text, tool_calls);
        changes.append_event(
            FactEvent::ToolCallRecorded {
                session: metadata,
                message: message_id,
                tool_call: call,
            },
            now_millis(),
        );
        self.commit(&changes).await?;
        Ok(())
    }

    /// 结束 run：写终态、assistant 终文本与 active leaf，并写入事实事件。
    ///
    /// # Errors
    ///
    /// - `StoreError::SessionNotFound`：session 不存在。
    /// - `StoreError::Database`：数据库读取或写入失败。
    /// - `StoreError::CorruptedData`：存储值不符合领域约束。
    /// - `StoreError::Serialization`：事实事件序列化失败。
    pub async fn finish_run(
        &self,
        session: &SessionId,
        run: &RunRecord,
        status: RunStatus,
        failure: Option<RunFailure>,
        assistant: MessageNode,
    ) -> Result<(), StoreError> {
        let _line = self.line.lock().await;
        let record = self.find_session(session).await?;
        let now = now_millis();
        let finished = RunRecord {
            status,
            failure,
            updated_at: now,
            ..run.clone()
        };
        let pending = self.storage.queue_length(session).await?;
        let metadata = record.metadata(pending, false);
        let mut changes = Changeset::new();
        changes.update_run(run.id, status, finished.failure.clone(), now);
        changes.update_message(
            assistant.id,
            assistant.text.clone(),
            assistant.tool_calls.clone(),
        );
        changes.update_session(
            session.clone(),
            record.title.clone(),
            record.title_auto,
            now,
            Some(assistant.id),
        );
        changes.append_event(
            FactEvent::MessageUpdated {
                session: metadata.clone(),
                message: assistant,
            },
            now,
        );
        changes.append_event(
            FactEvent::RunFinished {
                session: metadata,
                run: finished,
            },
            now,
        );
        self.commit(&changes).await?;
        Ok(())
    }

    /// 启动恢复：把仍为 running 的 run 标记为 failed（进程中断），并发布其事件。
    ///
    /// # Errors
    ///
    /// - `StoreError::SessionNotFound`：run 归属的 session 不存在。
    /// - `StoreError::Database`：数据库读取或写入失败。
    /// - `StoreError::CorruptedData`：存储值不符合领域约束。
    /// - `StoreError::Serialization`：事实事件序列化失败。
    pub async fn recover_running_runs(&self) -> Result<(), StoreError> {
        let _line = self.line.lock().await;
        let running = self.storage.running_runs().await?;
        if running.is_empty() {
            return Ok(());
        }
        tracing::warn!(runs = running.len(), "把中断的 run 标记为失败");
        let now = now_millis();
        let mut changes = Changeset::new();
        for run in &running {
            let failure = RunFailure {
                code: RunFailureCode::Interrupted,
                message: "agent 进程中断".to_owned(),
            };
            let finished = RunRecord {
                status: RunStatus::Failed,
                failure: Some(failure.clone()),
                updated_at: now,
                ..run.clone()
            };
            let session = self.find_session(&run.session_id).await?;
            let pending = self.storage.queue_length(&run.session_id).await?;
            changes.update_run(run.id, RunStatus::Failed, Some(failure), now);
            changes.update_session(
                run.session_id.clone(),
                session.title.clone(),
                session.title_auto,
                now,
                session.active_leaf,
            );
            changes.append_event(
                FactEvent::RunFinished {
                    session: session.metadata(pending, false),
                    run: finished,
                },
                now,
            );
        }
        self.commit(&changes).await?;
        Ok(())
    }
}
