//! run 的生命周期写入：开始、定型 assistant、工具节点三态与结束。
//! 调度与续跑点推导在 `crates/agent/src/engine`；队列项在此之前写入 `queue`。

use super::{Store, now_millis};
use crate::change::Changeset;
use crate::error::StoreError;
use crate::event::FactEvent;
use crate::id::{MessageId, RunId, SessionId};
use crate::message::{ContentPart, Message, MessageNode, MessageRole};
use crate::queue::QueueItem;
use crate::run::{RunFailure, RunRecord, RunStatus};
use crate::session::SessionMetadata;
use crate::store::RunStart;
use crate::tool::ToolResultState;

impl Store {
    /// 分配一个全局消息 id。定型前的 assistant 消息先用它作为实时增量的键。
    ///
    /// # Errors
    ///
    /// - `StoreError::Database`：数据库读取或写入失败。
    /// - `StoreError::CorruptedData`：分配出的 id 不符合领域约束。
    pub async fn mint_message_id(&self) -> Result<MessageId, StoreError> {
        self.storage.mint_message_id().await
    }

    /// 开始执行 session 队首的队列项：建用户节点、建 running 的 run、移动 active leaf。
    ///
    /// 队列项未锚定时在当前 active leaf 下新建用户节点；锚定用户消息时接续该节点，
    /// 未绑定 run 的补上关联；锚定其它角色时以它为起点，不新建用户节点。
    ///
    /// # Errors
    ///
    /// - `StoreError::SessionNotFound`：session 不存在。
    /// - `StoreError::MessageNotFound`：锚定的消息不是该 session 的消息。
    /// - `StoreError::Database`：数据库读取或写入失败。
    /// - `StoreError::CorruptedData`：存储值不符合领域约束。
    /// - `StoreError::Serialization`：事实事件序列化失败。
    ///
    /// 副作用：删除队首项，写入用户节点、run 行、active leaf 与事实事件。
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
        let start = self
            .run_start_node(&item, &run, session.active_leaf, &metadata, &mut changes)
            .await?;
        changes.update_session(
            id.clone(),
            session.title.clone(),
            session.title_auto,
            now,
            Some(start.id),
        );
        changes.append_event(
            FactEvent::ActiveLeafChanged {
                session: metadata.clone(),
                active_leaf: Some(start.id),
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
        }))
    }

    /// 登记 run 的起点节点：接续锚定的用户消息，锚定其它角色时以它为起点；
    /// 未锚定时新建挂在 `parent` 下的用户节点。
    async fn run_start_node(
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
                parts: vec![ContentPart::text(item.text.as_str())],
                run_id: Some(run.id),
                created_at: run.created_at,
            };
            changes.insert_message(Message {
                session: session.clone(),
                node: user.clone(),
            });
            changes.append_event(
                FactEvent::MessageAppended {
                    session: metadata.clone(),
                    message: user.skeleton(),
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
            return Ok(message.node);
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
                message: bound.skeleton(),
            },
            run.created_at,
        );
        Ok(bound)
    }

    /// 把一步模型回复定型入树：挂在当前 active leaf 下，块一次写齐。
    ///
    /// 输入：session、所属 run、步开始时铸造的消息 id 与全部内容块。
    ///
    /// # Errors
    ///
    /// - `StoreError::SessionNotFound`：session 不存在。
    /// - `StoreError::Database`：数据库读取或写入失败。
    /// - `StoreError::Serialization`：事实事件序列化失败。
    /// - `StoreError::Invariant`：run 的用户节点缺失。
    ///
    /// 副作用：写入 assistant 节点、active leaf 与 `message_appended` 事实事件。
    pub async fn append_assistant(
        &self,
        session: &SessionId,
        run_id: RunId,
        id: MessageId,
        parts: Vec<ContentPart>,
    ) -> Result<MessageNode, StoreError> {
        self.append_message(session, run_id, id, MessageRole::Assistant, parts)
            .await
    }

    /// 工具节点入树：状态为执行中，父节点为当前 active leaf。
    ///
    /// # Errors
    ///
    /// 同 [`Store::append_assistant`]。
    pub async fn append_tool(
        &self,
        session: &SessionId,
        run_id: RunId,
        id: MessageId,
        call_id: &str,
    ) -> Result<MessageNode, StoreError> {
        self.append_message(
            session,
            run_id,
            id,
            MessageRole::Tool,
            vec![ContentPart::tool_result(call_id, ToolResultState::Running)],
        )
        .await
    }

    async fn append_message(
        &self,
        session: &SessionId,
        run_id: RunId,
        id: MessageId,
        role: MessageRole,
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
            role,
            parts,
            run_id: Some(run_id),
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

    /// 写入工具结局：替换 tool 节点的全部块。
    ///
    /// # Errors
    ///
    /// - `StoreError::SessionNotFound`：session 不存在。
    /// - `StoreError::MessageNotFound`：tool 节点不存在。
    /// - `StoreError::MessageRoleMismatch`：目标不是 tool 节点。
    /// - `StoreError::Database`：数据库读取或写入失败。
    /// - `StoreError::Serialization`：事实事件序列化失败。
    ///
    /// 副作用：替换块、写入 `message_updated` 事实事件。
    pub async fn finish_tool(
        &self,
        session: &SessionId,
        id: MessageId,
        call_id: &str,
        state: ToolResultState,
    ) -> Result<MessageNode, StoreError> {
        let _line = self.line.lock().await;
        let record = self.find_session(session).await?;
        let stored = self
            .storage
            .message(session, id)
            .await?
            .ok_or_else(|| StoreError::MessageNotFound(session.clone(), id))?;
        if stored.node.role != MessageRole::Tool {
            return Err(StoreError::MessageRoleMismatch(id, stored.node.role));
        }
        let node = MessageNode {
            parts: vec![ContentPart::tool_result(call_id, state)],
            ..stored.node
        };
        let pending = self.storage.queue_length(session).await?;
        let metadata = record.metadata(pending, true);
        let now = now_millis();
        let mut changes = Changeset::new();
        changes.replace_message_parts(id, node.parts.clone());
        changes.append_event(
            FactEvent::MessageUpdated {
                session: metadata,
                message: node.skeleton(),
            },
            now,
        );
        self.commit(&changes).await?;
        Ok(node)
    }

    /// 结束 run：写终态并写入事实事件。
    ///
    /// # Errors
    ///
    /// - `StoreError::SessionNotFound`：session 不存在。
    /// - `StoreError::Database`：数据库读取或写入失败。
    /// - `StoreError::CorruptedData`：存储值不符合领域约束。
    /// - `StoreError::Serialization`：事实事件序列化失败。
    ///
    /// 副作用：更新 run 行、session 更新时间与 `run_finished` 事实事件。
    pub async fn finish_run(
        &self,
        session: &SessionId,
        run: &RunRecord,
        status: RunStatus,
        failure: Option<RunFailure>,
    ) -> Result<(), StoreError> {
        let _line = self.line.lock().await;
        let mut record = self.find_session(session).await?;
        let now = now_millis();
        let finished = RunRecord {
            status,
            failure,
            updated_at: now,
            ..run.clone()
        };
        record.updated_at = now;
        let pending = self.storage.queue_length(session).await?;
        let metadata = record.metadata(pending, false);
        let mut changes = Changeset::new();
        changes.update_run(run.id, status, finished.failure.clone(), now);
        changes.update_session(
            session.clone(),
            record.title.clone(),
            record.title_auto,
            now,
            record.active_leaf,
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

    /// 状态仍是 running 的全部 run，按 id 升序。启动恢复据此续跑。
    ///
    /// # Errors
    ///
    /// - `StoreError::Database`：数据库读取失败。
    /// - `StoreError::CorruptedData`：存储值不符合领域约束。
    pub async fn running_runs(&self) -> Result<Vec<RunRecord>, StoreError> {
        self.storage.running_runs().await
    }

    /// 该 session 仍为 running 的 run，最多一条。
    ///
    /// # Errors
    ///
    /// - `StoreError::Database`：数据库读取失败。
    /// - `StoreError::CorruptedData`：存储值不符合领域约束。
    pub async fn running_run(&self, session: &SessionId) -> Result<Option<RunRecord>, StoreError> {
        self.storage.running_run(session).await
    }
}
