//! run 的生命周期写入：开始、定型 assistant、工具节点三态与结束。
//! 调度与续跑点推导在 `crates/agent/src/engine`；队列项在此之前写入 `queue`。

use super::{Store, append_node};
use crate::change::Changeset;
use crate::error::StoreError;
use crate::event::FactEvent;
use crate::id::{MessageId, RunId, SessionId};
use crate::message::{ContentPart, Message, MessageNode, MessageRole};
use crate::queue::QueueItem;
use crate::run::{RunFailure, RunRecord, RunStatus};
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
        self.command(id, async |command| {
            if command.busy() {
                return Ok(None);
            }
            let Some(item) = self.storage.first_queue_item(id).await? else {
                return Ok(None);
            };
            let now = command.now();
            let run = RunRecord {
                id: self.storage.mint_run_id().await?,
                session_id: id.clone(),
                status: RunStatus::Running,
                failure: None,
                created_at: now,
                updated_at: now,
            };
            command.changes().delete_queue_item(id.clone(), item.id);
            command.changes().insert_run(run.clone());
            let start = self
                .run_start_node(
                    &item,
                    &run,
                    command.session().active_leaf,
                    command.changes(),
                )
                .await?;
            command.session_mut().active_leaf = Some(start.id);
            command.changes().append_event(
                FactEvent::ActiveLeafChanged {
                    session_id: id.clone(),
                    active_leaf: Some(start.id),
                },
                now,
            );
            command.changes().append_event(
                FactEvent::QueueItemRemoved {
                    session_id: id.clone(),
                    item_id: item.id,
                },
                now,
            );
            command.changes().append_event(
                FactEvent::RunStarted {
                    session_id: id.clone(),
                    run: run.clone(),
                },
                now,
            );
            Ok(Some(RunStart {
                run,
                workspace: command.session().workspace().clone(),
            }))
        })
        .await
    }

    /// 登记 run 的起点节点：接续锚定的用户消息，锚定其它角色时以它为起点；
    /// 未锚定时新建挂在 `parent` 下的用户节点。
    async fn run_start_node(
        &self,
        item: &QueueItem,
        run: &RunRecord,
        parent: Option<MessageId>,
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
                    session_id: session.clone(),
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
        // 插话降级后可能已随链进入过后续请求：再接续它会把 active leaf 拽回，
        // 丢掉后来的消息。这类锚点从链尾接续。
        if let Some(tail) = self.chain_tail(session, anchor).await? {
            return Ok(tail);
        }
        let bound = MessageNode {
            run_id: Some(run.id),
            ..message.node
        };
        changes.bind_message_run(bound.id, run.id);
        changes.append_event(
            FactEvent::MessageUpdated {
                session_id: session.clone(),
                message: bound.skeleton(),
            },
            run.created_at,
        );
        Ok(bound)
    }

    /// 锚定的用户节点已在链上但不是链尾时返回链尾节点。
    ///
    /// 它的正文已经随链发给过模型，因此下一次 run 从链尾接续，不把 active leaf 拽回。
    async fn chain_tail(
        &self,
        id: &SessionId,
        anchor: MessageId,
    ) -> Result<Option<MessageNode>, StoreError> {
        let chain = self.storage.transcript(id).await?;
        let Some(position) = chain.iter().position(|node| node.id == anchor) else {
            return Ok(None);
        };
        if chain.get(position + 1).is_none() {
            return Ok(None);
        }
        Ok(chain.last().cloned())
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
        self.command(session, async |command| {
            let now = command.now();
            let node = MessageNode {
                id,
                parent_id: command.session().active_leaf,
                role,
                parts,
                run_id: Some(run_id),
                created_at: now,
            };
            append_node(command, &node);
            Ok(node)
        })
        .await
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
        self.command(session, async |command| {
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
            let now = command.now();
            command
                .changes()
                .replace_message_parts(id, node.parts.clone());
            command.changes().append_event(
                FactEvent::MessageUpdated {
                    session_id: session.clone(),
                    message: node.skeleton(),
                },
                now,
            );
            Ok(node)
        })
        .await
    }

    /// 结束 run：写终态并写入事实事件。
    ///
    /// # Errors
    ///
    /// - `StoreError::SessionNotFound`：session 不存在。
    /// - `StoreError::Database`：数据库读取或写入失败。
    /// - `StoreError::CorruptedData`：存储值不符合领域约束。
    /// - `StoreError::Serialization`：事实事件序列化失败。
    /// - `StoreError::Invariant`：run 行未命中，说明 run 不属于该 session。
    ///
    /// 副作用：更新 run 行与 `run_finished` 事实事件。
    pub async fn finish_run(
        &self,
        session: &SessionId,
        run: &RunRecord,
        status: RunStatus,
        failure: Option<RunFailure>,
    ) -> Result<(), StoreError> {
        self.command(session, async |command| {
            let now = command.now();
            let finished = RunRecord {
                status,
                failure,
                updated_at: now,
                ..run.clone()
            };
            command
                .changes()
                .update_run(run.id, status, finished.failure.clone(), now);
            command.changes().append_event(
                FactEvent::RunFinished {
                    session_id: session.clone(),
                    run: finished,
                },
                now,
            );
            Ok(())
        })
        .await
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
