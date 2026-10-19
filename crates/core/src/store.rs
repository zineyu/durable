use std::path::Path;

use tokio::sync::Mutex;

use crate::change::Changeset;
use crate::error::StoreError;
use crate::event::{CommittedEvent, FactEvent};
use crate::id::{MessageId, QueueItemId, SessionId, SessionTitle, WorkspacePath};
use crate::message::{Message, MessageContent, MessageNode, MessageRole, MessageText};
use crate::queue::QueueItem;
use crate::run::{RunFailure, RunFailureCode, RunRecord, RunStatus};
use crate::session::{SessionMetadata, SessionRecord, SessionTree, derive_title};
use crate::storage::Snapshot;
use crate::storage::sqlite::SqliteStorage;
use crate::stream::{EventBus, StreamEvent};
use crate::tool::ToolCallRecord;
use crate::workspace::WorkspaceMetadata;

/// 一次 run 开始时建立的节点与记录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunStart {
    pub run: RunRecord,
    pub workspace: WorkspacePath,
    pub user: MessageNode,
    pub assistant: MessageNode,
}

/// 领域内核：唯一写入线 + 持久化边界。
///
/// 所有写入经写入线串行化，读-改-写之间不会被其它写入插入。每次写入以一个
/// [`Changeset`] 在一次存储事务内生效；事务成功前不发布任何事实事件。
pub struct Store {
    storage: SqliteStorage,
    line: Mutex<()>,
    bus: EventBus,
}

impl Store {
    /// 打开 SQLite 文件并应用迁移。
    ///
    /// 失败：数据库无法打开、连接参数被拒绝或迁移失败。
    /// 副作用：创建数据库文件与表。
    pub async fn open(path: &Path) -> Result<Self, StoreError> {
        Ok(Self {
            storage: SqliteStorage::open(path).await?,
            line: Mutex::new(()),
            bus: EventBus::default(),
        })
    }

    /// 进程内事件总线。事实事件在提交成功后发布。
    pub fn events(&self) -> &EventBus {
        &self.bus
    }

    /// 读取 bootstrap 元数据。
    ///
    /// 失败：数据库读取失败或存储值不符合领域约束。
    pub async fn bootstrap(&self) -> Result<Snapshot, StoreError> {
        self.storage.snapshot().await
    }

    /// 注册 workspace。
    ///
    /// 输入：已规范化的 workspace 身份路径。
    /// 失败：该路径已注册。
    /// 副作用：写入 workspace 行与 `workspace_registered` 事实事件。
    pub async fn register_workspace(
        &self,
        path: &WorkspacePath,
    ) -> Result<WorkspaceMetadata, StoreError> {
        let _line = self.line.lock().await;
        if self.storage.workspace(path).await?.is_some() {
            return Err(StoreError::WorkspaceDuplicate(path.clone()));
        }
        self.insert_workspace(path).await
    }

    /// 注册启动参数给出的初始 workspace。
    ///
    /// 输入：已规范化的 workspace 路径列表；重复出现与已注册的路径被跳过。
    /// 失败：数据库读取或写入失败。
    /// 副作用：为新路径写入 workspace 行与事实事件。
    pub async fn register_initial_workspaces(
        &self,
        paths: &[WorkspacePath],
    ) -> Result<(), StoreError> {
        let _line = self.line.lock().await;
        for path in paths {
            if self.storage.workspace(path).await?.is_some() {
                continue;
            }
            self.insert_workspace(path).await?;
        }
        Ok(())
    }

    async fn insert_workspace(
        &self,
        path: &WorkspacePath,
    ) -> Result<WorkspaceMetadata, StoreError> {
        let registered_at = now_millis();
        let workspace = WorkspaceMetadata {
            path: path.clone(),
            registered_at,
        };
        let mut changes = Changeset::new();
        changes.insert_workspace(workspace.clone());
        changes.append_event(
            FactEvent::WorkspaceRegistered {
                workspace: workspace.clone(),
            },
            registered_at,
        );
        self.commit(&changes).await?;
        Ok(workspace)
    }

    /// 移除 workspace 注册记录。
    ///
    /// 输入：存储中的 workspace 身份路径；不访问文件系统，因此目录已被删除时仍可移除。
    /// 失败：该路径未注册，或该 workspace 仍有 session。
    /// 副作用：删除 workspace 行，写入 `workspace_removed` 事实事件。
    pub async fn remove_workspace(&self, path: &WorkspacePath) -> Result<(), StoreError> {
        let _line = self.line.lock().await;
        if self.storage.workspace(path).await?.is_none() {
            return Err(StoreError::WorkspaceNotFound(path.clone()));
        }
        if self.storage.session_count(path).await? > 0 {
            return Err(StoreError::WorkspaceNotEmpty(path.clone()));
        }
        let mut changes = Changeset::new();
        changes.delete_workspace(path.clone());
        changes.append_event(
            FactEvent::WorkspaceRemoved { path: path.clone() },
            now_millis(),
        );
        self.commit(&changes).await.map(|_| ())
    }

    /// 在指定 workspace 下创建 session。
    ///
    /// 输入：已规范化的 workspace 路径与已解析的标题。
    /// 失败：该 workspace 未注册。
    /// 副作用：写入 session 行与 `session_created` 事实事件。
    pub async fn create_session(
        &self,
        workspace: &WorkspacePath,
        title: SessionTitle,
    ) -> Result<SessionMetadata, StoreError> {
        let _line = self.line.lock().await;
        if self.storage.workspace(workspace).await?.is_none() {
            return Err(StoreError::WorkspaceNotFound(workspace.clone()));
        }
        let created_at = now_millis();
        let session = SessionRecord {
            id: SessionId::generate(),
            workspace: workspace.clone(),
            title,
            title_auto: true,
            created_at,
            updated_at: created_at,
            active_leaf: None,
        };
        let mut changes = Changeset::new();
        changes.insert_session(session.clone());
        changes.append_event(
            FactEvent::SessionCreated {
                session: session.metadata(0, false),
            },
            created_at,
        );
        self.commit(&changes).await?;
        Ok(session.metadata(0, false))
    }

    /// 重命名 session。
    ///
    /// 输入：session 身份与已解析的标题。
    /// 失败：session 不存在。
    /// 副作用：更新标题与 `updated_at`，写入 `session_renamed` 事实事件。
    pub async fn rename_session(
        &self,
        id: &SessionId,
        title: &SessionTitle,
    ) -> Result<SessionMetadata, StoreError> {
        let _line = self.line.lock().await;
        let mut session = self.find_session(id).await?;
        session.title = title.clone();
        session.title_auto = false;
        session.updated_at = now_millis();
        let pending = self.storage.queue_length(id).await?;
        let busy = self.storage.busy(id).await?;
        let mut changes = Changeset::new();
        changes.update_session(
            session.id.clone(),
            session.title.clone(),
            session.title_auto,
            session.updated_at,
            session.active_leaf,
        );
        changes.append_event(
            FactEvent::SessionRenamed {
                session: session.metadata(pending, busy),
            },
            session.updated_at,
        );
        self.commit(&changes).await?;
        Ok(session.metadata(pending, busy))
    }

    /// 删除 session。
    ///
    /// 输入：session 身份。
    /// 失败：session 不存在。
    /// 副作用：删除该 session 的消息树、队列、run 与历史事件，再写入 `session_deleted` 事实事件。
    /// 不访问 workspace 文件系统。
    pub async fn delete_session(&self, id: &SessionId) -> Result<(), StoreError> {
        let _line = self.line.lock().await;
        self.find_session(id).await?;
        let mut changes = Changeset::new();
        changes.delete_session(id.clone());
        changes.append_event(
            FactEvent::SessionDeleted {
                session_id: id.clone(),
            },
            now_millis(),
        );
        self.commit(&changes).await.map(|_| ())
    }

    /// 读取单个 session 的消息树。
    ///
    /// 失败：session 不存在。
    pub async fn session_tree(&self, id: &SessionId) -> Result<SessionTree, StoreError> {
        self.storage
            .session_tree(id)
            .await?
            .map(snapshot_into_tree)
            .ok_or_else(|| StoreError::SessionNotFound(id.clone()))
    }

    /// 读取 session 的 transcript：root 到 active leaf 的路径。
    ///
    /// 失败：session 不存在。
    pub async fn transcript(&self, id: &SessionId) -> Result<Vec<MessageNode>, StoreError> {
        let tree = self.session_tree(id).await?;
        Ok(path_to_leaf(&tree.nodes, tree.active_leaf))
    }

    /// 把一条用户消息写入 session 的 FIFO 队列。此阶段不创建消息节点，不移动 active leaf。
    ///
    /// 输入：session 身份与已校验的消息正文。
    /// 失败：session 不存在。
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
            session.title = derive_title(&text);
        }
        session.updated_at = created_at;
        let item = QueueItem {
            id: self.storage.mint_queue_item_id().await?,
            text,
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
    /// 失败：session 不存在，或该项不在该 session 的队列中。
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

    /// 开始执行 session 队首的用户消息：建两个消息节点、建 running 的 run、移动 active leaf。
    ///
    /// 失败：session 不存在。
    /// 副作用：删除队首项，写入用户与 assistant 节点、run 行与事实事件。
    /// 队列为空或该 session 已有 running 的 run 时返回 `None`。
    pub async fn start_run(&self, id: &SessionId) -> Result<Option<RunStart>, StoreError> {
        let _line = self.line.lock().await;
        let session = self.find_session(id).await?;
        if self.storage.busy(id).await? {
            return Ok(None);
        }
        let Some(item) = self.storage.first_queue_item(id).await? else {
            return Ok(None);
        };
        let now = now_millis();
        let run_id = self.storage.mint_run_id().await?;
        let user_id = self.storage.mint_message_id().await?;
        let assistant_id = self.storage.mint_message_id().await?;
        let user = MessageNode {
            id: user_id,
            parent_id: session.active_leaf,
            role: MessageRole::User,
            text: MessageContent::new(item.text.as_str()),
            run_id: Some(run_id),
            tool_calls: Vec::new(),
            created_at: now,
        };
        let assistant = MessageNode {
            id: assistant_id,
            parent_id: Some(user_id),
            role: MessageRole::Assistant,
            text: MessageContent::empty(),
            run_id: Some(run_id),
            tool_calls: Vec::new(),
            created_at: now,
        };
        let run = RunRecord {
            id: run_id,
            session_id: id.clone(),
            status: RunStatus::Running,
            failure: None,
            created_at: now,
            updated_at: now,
        };
        let pending = self.storage.queue_length(id).await?;
        let remaining = pending.saturating_sub(1);
        let metadata = session.metadata(remaining, true);
        let mut changes = Changeset::new();
        changes.delete_queue_item(id.clone(), item.id);
        changes.insert_run(run.clone());
        changes.insert_message(Message {
            session: id.clone(),
            node: user.clone(),
        });
        changes.insert_message(Message {
            session: id.clone(),
            node: assistant.clone(),
        });
        changes.update_session(
            id.clone(),
            session.title.clone(),
            session.title_auto,
            now,
            Some(assistant_id),
        );
        changes.append_event(
            FactEvent::MessageAppended {
                session: metadata.clone(),
                message: user.clone(),
            },
            now,
        );
        changes.append_event(
            FactEvent::MessageAppended {
                session: metadata.clone(),
                message: assistant.clone(),
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

    /// 节流写入 assistant 节点的部分输出。此写入不产生事实事件。
    ///
    /// 失败：该消息不存在。
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

    /// 结束 run：写终态、assistant 终文本与 active leaf，并写入事实事件。
    ///
    /// 失败：session 或 run 不存在。
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
    /// 失败：数据库读取或写入失败。
    pub async fn recover_running_runs(&self) -> Result<(), StoreError> {
        let _line = self.line.lock().await;
        let running = self.storage.running_runs().await?;
        if running.is_empty() {
            return Ok(());
        }
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

    /// session 是否有 running 的 run。
    pub async fn busy(&self, id: &SessionId) -> Result<bool, StoreError> {
        self.storage.busy(id).await
    }

    /// 队列非空的全部 session。
    pub async fn sessions_with_queue(&self) -> Result<Vec<SessionId>, StoreError> {
        self.storage.sessions_with_queue().await
    }

    /// 从 `after` 游标之后回放事实事件。
    pub async fn events_after(&self, after: i64) -> Result<Vec<CommittedEvent>, StoreError> {
        self.storage.events_after(after).await
    }

    /// 读取 session 记录。
    ///
    /// 失败：session 不存在。
    pub async fn session_record(&self, id: &SessionId) -> Result<SessionRecord, StoreError> {
        self.find_session(id).await
    }

    async fn find_session(&self, id: &SessionId) -> Result<SessionRecord, StoreError> {
        self.storage
            .session(id)
            .await?
            .ok_or_else(|| StoreError::SessionNotFound(id.clone()))
    }

    /// 提交一次变更；事务成功后才发布事实事件。
    async fn commit(&self, changes: &Changeset) -> Result<Vec<CommittedEvent>, StoreError> {
        let committed = self.storage.apply(changes).await?;
        for event in &committed {
            self.bus.publish(StreamEvent::Fact {
                id: event.id,
                session_id: event.event.session_id().cloned(),
                timestamp: event.created_at,
                event: event.event.clone(),
            });
        }
        Ok(committed)
    }
}

fn snapshot_into_tree(snapshot: crate::storage::SessionSnapshot) -> SessionTree {
    SessionTree {
        session: snapshot
            .session
            .metadata(snapshot.queue_length, snapshot.busy),
        active_leaf: snapshot.session.active_leaf,
        nodes: snapshot
            .messages
            .into_iter()
            .map(|message| message.node)
            .collect(),
        queue: snapshot.queue,
        runs: snapshot.runs,
    }
}

/// 从全部节点中取出 root 到 `leaf` 的路径，顺序为 root 在前。
fn path_to_leaf(nodes: &[MessageNode], leaf: Option<MessageId>) -> Vec<MessageNode> {
    let Some(leaf) = leaf else {
        return Vec::new();
    };
    let mut path = Vec::new();
    let mut current = Some(leaf);
    while let Some(id) = current {
        let Some(node) = nodes.iter().find(|node| node.id == id) else {
            break;
        };
        path.push(node.clone());
        current = node.parent_id;
    }
    path.reverse();
    path
}

fn now_millis() -> i64 {
    jiff::Timestamp::now().as_millisecond()
}
