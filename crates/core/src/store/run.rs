//! run 的生命周期写入：开始、定型 assistant、工具节点三态、流式进度与结束。
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
                progress: None,
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
                    run: Box::new(run.clone()),
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
    /// 未锚定时新建挂在当前叶子 `leaf` 下的用户节点。
    async fn run_start_node(
        &self,
        item: &QueueItem,
        run: &RunRecord,
        leaf: Option<MessageId>,
        changes: &mut Changeset,
    ) -> Result<MessageNode, StoreError> {
        let session = &run.session_id;
        let Some(anchor) = item.anchor else {
            let user = MessageNode {
                id: self.storage.mint_message_id().await?,
                parent_id: leaf,
                role: MessageRole::User,
                interrupted: false,
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
        if let Some(tail) = self.chain_tail(session, leaf, anchor).await? {
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
    /// `leaf` 是本次 run 开始前的 active leaf，也是 run 起点节点的挂载点。
    /// 锚定节点的正文已经随链发给过模型，因此下一次 run 从链尾接续，不把 active leaf 拽回。
    async fn chain_tail(
        &self,
        id: &SessionId,
        leaf: Option<MessageId>,
        anchor: MessageId,
    ) -> Result<Option<MessageNode>, StoreError> {
        let Some(leaf) = leaf else {
            return Ok(None);
        };
        let tree = self.message_tree(id).await?;
        let chain = tree.transcript(leaf);
        if !chain.iter().any(|node| node.id == anchor) {
            return Ok(None);
        }
        let Some(tail) = chain.last() else {
            return Ok(None);
        };
        if tail.id == anchor {
            return Ok(None);
        }
        let stored = self
            .storage
            .message(id, tail.id)
            .await?
            .ok_or_else(|| StoreError::MessageNotFound(id.clone(), tail.id))?;
        Ok(Some(stored.node))
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
                interrupted: false,
                parts,
                run_id: Some(run_id),
                created_at: now,
            };
            append_node(command, &node);
            // 定型与清空进度同一事务：不存在「消息已入树、进度仍残留」的状态。
            if role == MessageRole::Assistant {
                command.changes().update_run_progress(run_id, None);
            }
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

    /// 写入 run 的流式进度。只存正文，不含推理与未完成的工具调用。
    ///
    /// 正文与已存值相同时不提交：不写行、不产生事实事件、不推进 session 的更新时间。
    ///
    /// # Errors
    ///
    /// - `StoreError::SessionNotFound`：session 不存在。
    /// - `StoreError::Invariant`：该 session 没有执行中的 run，或 run 不是给定的那条。
    /// - `StoreError::Database`：数据库读取或写入失败。
    pub async fn write_progress(
        &self,
        session: &SessionId,
        run: RunId,
        text: &str,
    ) -> Result<(), StoreError> {
        if text.is_empty() {
            return Ok(());
        }
        self.command(session, async |command| {
            let running = self
                .running_run(session)
                .await?
                .ok_or_else(|| not_running(run))?;
            if running.id != run {
                return Err(not_running(run));
            }
            if running.progress.as_deref() == Some(text) {
                return Ok(());
            }
            command.changes().update_run_progress(run, Some(text));
            Ok(())
        })
        .await
    }

    /// 把执行中的 run 的进度定型为一条已中断的 assistant 消息，并在同一事务里清空进度。
    ///
    /// 启动恢复与用户取消共用这个转换点。进度为空时什么也不写，因此恢复途中再次被杀
    /// 也不会写出第二条消息。
    ///
    /// # Errors
    ///
    /// - `StoreError::SessionNotFound`：session 不存在。
    /// - `StoreError::Database`：数据库读取或写入失败。
    /// - `StoreError::Serialization`：事实事件序列化失败。
    pub async fn materialize_progress(
        &self,
        session: &SessionId,
    ) -> Result<Option<MessageNode>, StoreError> {
        self.command(session, async |command| {
            let Some(running) = self.running_run(session).await? else {
                return Ok(None);
            };
            let Some(text) = running.progress.clone() else {
                return Ok(None);
            };
            let node = MessageNode {
                id: self.storage.mint_message_id().await?,
                parent_id: command.session().active_leaf,
                role: MessageRole::Assistant,
                interrupted: true,
                parts: vec![ContentPart::text(text)],
                run_id: Some(running.id),
                created_at: command.now(),
            };
            append_node(command, &node);
            command.changes().update_run_progress(running.id, None);
            Ok(Some(node))
        })
        .await
    }

    /// 结束 run：写终态并写入事实事件。终态与清空进度在同一事务里生效。
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
                progress: None,
                updated_at: now,
                ..run.clone()
            };
            command
                .changes()
                .update_run(run.id, status, finished.failure.clone(), now);
            command.changes().update_run_progress(run.id, None);
            command.changes().append_event(
                FactEvent::RunFinished {
                    session_id: session.clone(),
                    run: Box::new(finished),
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

fn not_running(run: RunId) -> StoreError {
    StoreError::Invariant(format!("run {} 不在执行中", run.get()))
}
