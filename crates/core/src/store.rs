use std::path::Path;

use tokio::sync::Mutex;

use crate::change::Changeset;
use crate::error::StoreError;
use crate::event::{CommittedEvent, FactEvent};
use crate::id::{MessageId, QueueItemId, SessionId, SessionTitle, WorkspacePath};
use crate::message::{Message, MessageContent, MessageNode, MessageRole, MessageText};
use crate::node::{NodeMetadata, NodeName};
use crate::queue::QueueItem;
use crate::run::{RunFailure, RunFailureCode, RunRecord, RunStatus};
use crate::session::{SessionMetadata, SessionRecord, SessionTree, derive_title};
use crate::storage::sqlite::SqliteStorage;
use crate::storage::{SessionSnapshot, Snapshot};
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
    /// 打开 SQLite 文件并应用迁移。父目录不存在时自动建立。
    ///
    /// 失败：目录无法建立、数据库无法打开、连接参数被拒绝或迁移失败。
    /// 副作用：创建数据库目录、数据库文件与表。
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

    /// 节点表中的单个节点。
    ///
    /// 失败：数据库读取失败或存储值不符合领域约束。
    pub async fn node(&self, name: &NodeName) -> Result<Option<NodeMetadata>, StoreError> {
        self.storage.node(name).await
    }

    /// 节点表中的全部节点，按注册顺序排列。
    ///
    /// 失败：数据库读取失败或存储值不符合领域约束。
    pub async fn nodes(&self) -> Result<Vec<NodeMetadata>, StoreError> {
        self.storage.nodes().await
    }

    /// 注册节点。
    ///
    /// 输入：已解析的节点名与非空 URL。
    /// 失败：该名称已注册。
    /// 副作用：写入 node 行。
    pub async fn register_node(
        &self,
        name: NodeName,
        url: String,
    ) -> Result<NodeMetadata, StoreError> {
        let _line = self.line.lock().await;
        if self.storage.node(&name).await?.is_some() {
            return Err(StoreError::NodeNameTaken(name));
        }
        let node = NodeMetadata {
            name,
            url,
            created_at: now_millis(),
        };
        let mut changes = Changeset::new();
        changes.insert_node(node.clone());
        self.commit(&changes).await?;
        Ok(node)
    }

    /// 重命名节点。
    ///
    /// 输入：节点表中的旧名与新名。名称即身份，改名后旧名不再存在。
    /// 失败：旧名不在节点表中，或新名已被使用。
    /// 副作用：替换 node 行，保留 URL 与注册时间。
    pub async fn rename_node(
        &self,
        name: &NodeName,
        next: NodeName,
    ) -> Result<NodeMetadata, StoreError> {
        let _line = self.line.lock().await;
        let Some(node) = self.storage.node(name).await? else {
            return Err(StoreError::NodeNotFound(name.clone()));
        };
        if *name == next {
            return Ok(node);
        }
        if self.storage.node(&next).await?.is_some() {
            return Err(StoreError::NodeNameTaken(next));
        }
        let renamed = NodeMetadata {
            name: next,
            url: node.url,
            created_at: node.created_at,
        };
        let mut changes = Changeset::new();
        changes.delete_node(name.clone());
        changes.insert_node(renamed.clone());
        self.commit(&changes).await?;
        Ok(renamed)
    }

    /// 移除节点。
    ///
    /// 失败：该名称不在节点表中。
    /// 副作用：删除 node 行。
    pub async fn remove_node(&self, name: &NodeName) -> Result<(), StoreError> {
        let _line = self.line.lock().await;
        if self.storage.node(name).await?.is_none() {
            return Err(StoreError::NodeNotFound(name.clone()));
        }
        let mut changes = Changeset::new();
        changes.delete_node(name.clone());
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

    /// 编辑一条历史用户消息：在其父节点下创建兄弟用户节点，把 active leaf 移到新节点并入队执行。
    ///
    /// 输入：session 身份、目标用户消息身份与已校验的新正文。
    /// 失败：session 不存在，目标不是该 session 的用户消息，或 session 忙碌、队列非空。
    /// 副作用：写入新用户节点、锚定该节点的队列项与 active leaf，并写入事实事件。
    pub async fn edit_user_message(
        &self,
        id: &SessionId,
        target: MessageId,
        text: MessageText,
    ) -> Result<MessageNode, StoreError> {
        let _line = self.line.lock().await;
        let mut session = self.find_session(id).await?;
        self.ensure_idle(id).await?;
        let snapshot = self.session_snapshot(id).await?;
        let target_node = find_node(&snapshot.messages, target)
            .ok_or_else(|| StoreError::MessageNotFound(id.clone(), target))?;
        if target_node.role != MessageRole::User {
            return Err(StoreError::MessageRoleMismatch(target, target_node.role));
        }
        let created_at = now_millis();
        let node = MessageNode {
            id: self.storage.mint_message_id().await?,
            parent_id: target_node.parent_id,
            role: MessageRole::User,
            text: MessageContent::new(text.as_str()),
            run_id: None,
            tool_calls: Vec::new(),
            created_at,
        };
        let item = QueueItem {
            id: self.storage.mint_queue_item_id().await?,
            text,
            anchor: Some(node.id),
            created_at,
        };
        session.updated_at = created_at;
        let metadata = session.metadata(1, false);
        let mut changes = Changeset::new();
        changes.insert_message(Message {
            session: id.clone(),
            node: node.clone(),
        });
        changes.insert_queue_item(id.clone(), item.clone());
        changes.update_session(
            id.clone(),
            session.title.clone(),
            session.title_auto,
            created_at,
            Some(node.id),
        );
        changes.append_event(
            FactEvent::MessageAppended {
                session: metadata.clone(),
                message: node.clone(),
            },
            created_at,
        );
        changes.append_event(
            FactEvent::MessageQueued {
                session: metadata,
                item,
            },
            created_at,
        );
        self.commit(&changes).await?;
        Ok(node)
    }

    /// 重试一条 assistant 消息：以其父用户节点为锚入队，run 开始时创建同父兄弟 assistant 节点。
    ///
    /// 输入：session 身份与目标 assistant 消息身份。
    /// 失败：session 不存在，目标不是该 session 的 assistant 消息，或 session 忙碌、队列非空。
    /// 副作用：写入锚定父用户节点的队列项与更新时间；不创建用户节点。
    pub async fn retry_assistant_message(
        &self,
        id: &SessionId,
        target: MessageId,
    ) -> Result<(), StoreError> {
        let _line = self.line.lock().await;
        let mut session = self.find_session(id).await?;
        self.ensure_idle(id).await?;
        let snapshot = self.session_snapshot(id).await?;
        let target_node = find_node(&snapshot.messages, target)
            .ok_or_else(|| StoreError::MessageNotFound(id.clone(), target))?;
        if target_node.role != MessageRole::Assistant {
            return Err(StoreError::MessageRoleMismatch(target, target_node.role));
        }
        let parent_id = target_node.parent_id.ok_or_else(|| {
            StoreError::Invariant(format!("assistant 消息 {} 缺少父节点", target.get()))
        })?;
        let parent = find_node(&snapshot.messages, parent_id)
            .ok_or_else(|| StoreError::MessageNotFound(id.clone(), parent_id))?;
        let text = MessageText::parse(parent.text.as_str())
            .map_err(|error| StoreError::CorruptedData(format!("message.text: {error}")))?;
        let created_at = now_millis();
        let item = QueueItem {
            id: self.storage.mint_queue_item_id().await?,
            text,
            anchor: Some(parent_id),
            created_at,
        };
        session.updated_at = created_at;
        let metadata = session.metadata(1, false);
        let mut changes = Changeset::new();
        changes.insert_queue_item(id.clone(), item.clone());
        changes.update_session(
            id.clone(),
            session.title.clone(),
            session.title_auto,
            created_at,
            session.active_leaf,
        );
        changes.append_event(
            FactEvent::MessageQueued {
                session: metadata,
                item,
            },
            created_at,
        );
        self.commit(&changes).await?;
        Ok(())
    }

    /// 切换 active leaf：把 active leaf 移到目标子树中最近创建的叶子。
    ///
    /// 输入：session 身份与目标消息身份。
    /// 失败：session 不存在，目标不属于该 session，或 session 忙碌、队列非空。
    /// 副作用：更新 active leaf 与更新时间，写入 `active_leaf_changed` 事实事件。
    pub async fn switch_active_leaf(
        &self,
        id: &SessionId,
        target: MessageId,
    ) -> Result<MessageId, StoreError> {
        let _line = self.line.lock().await;
        let mut session = self.find_session(id).await?;
        self.ensure_idle(id).await?;
        let snapshot = self.session_snapshot(id).await?;
        let nodes: Vec<MessageNode> = snapshot
            .messages
            .iter()
            .map(|message| message.node.clone())
            .collect();
        if !nodes.iter().any(|node| node.id == target) {
            return Err(StoreError::MessageNotFound(id.clone(), target));
        }
        let leaf = branch_leaf(&nodes, target).ok_or_else(|| {
            StoreError::Invariant(format!("目标 {} 的子树没有叶子", target.get()))
        })?;
        let now = now_millis();
        let metadata = session.metadata(snapshot.queue_length, snapshot.busy);
        session.updated_at = now;
        let mut changes = Changeset::new();
        changes.update_session(
            id.clone(),
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
        self.commit(&changes).await?;
        Ok(leaf)
    }

    /// 分支命令的前置条件：session 空闲且队列为空。队列非空优先于 run 进行中报告。
    async fn ensure_idle(&self, id: &SessionId) -> Result<(), StoreError> {
        if self.storage.queue_length(id).await? > 0 {
            return Err(StoreError::QueueNotEmpty(id.clone()));
        }
        if self.storage.busy(id).await? {
            return Err(StoreError::RunInProgress(id.clone()));
        }
        Ok(())
    }

    async fn session_snapshot(&self, id: &SessionId) -> Result<SessionSnapshot, StoreError> {
        self.storage
            .session_tree(id)
            .await?
            .ok_or_else(|| StoreError::SessionNotFound(id.clone()))
    }

    /// 开始执行 session 队首的队列项：建 assistant 节点、建 running 的 run、移动 active leaf。
    ///
    /// 队列项未锚定用户节点时在当前 active leaf 下新建用户节点；已锚定时接续该节点，
    /// 已存在但未绑定 run 的节点补上 run 关联。
    ///
    /// 失败：session 不存在或锚定的消息与 session 不匹配。
    /// 副作用：删除队首项，写入消息节点、run 行、active leaf 与事实事件。
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
        let user = match item.anchor {
            Some(anchor) => {
                let message = self
                    .storage
                    .message(id, anchor)
                    .await?
                    .ok_or_else(|| StoreError::MessageNotFound(id.clone(), anchor))?;
                if message.node.role != MessageRole::User {
                    return Err(StoreError::MessageRoleMismatch(anchor, message.node.role));
                }
                if message.node.run_id.is_some() {
                    message.node
                } else {
                    let bound = MessageNode {
                        run_id: Some(run_id),
                        ..message.node
                    };
                    changes.bind_message_run(bound.id, run_id);
                    changes.append_event(
                        FactEvent::MessageUpdated {
                            session: metadata.clone(),
                            message: bound.clone(),
                        },
                        now,
                    );
                    bound
                }
            }
            None => {
                let user = MessageNode {
                    id: self.storage.mint_message_id().await?,
                    parent_id: session.active_leaf,
                    role: MessageRole::User,
                    text: MessageContent::new(item.text.as_str()),
                    run_id: Some(run_id),
                    tool_calls: Vec::new(),
                    created_at: now,
                };
                changes.insert_message(Message {
                    session: id.clone(),
                    node: user.clone(),
                });
                changes.append_event(
                    FactEvent::MessageAppended {
                        session: metadata.clone(),
                        message: user.clone(),
                    },
                    now,
                );
                user
            }
        };
        let assistant = MessageNode {
            id: self.storage.mint_message_id().await?,
            parent_id: Some(user.id),
            role: MessageRole::Assistant,
            text: MessageContent::empty(),
            run_id: Some(run_id),
            tool_calls: Vec::new(),
            created_at: now,
        };
        changes.insert_message(Message {
            session: id.clone(),
            node: assistant.clone(),
        });
        changes.update_session(
            id.clone(),
            session.title.clone(),
            session.title_auto,
            now,
            Some(assistant.id),
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

    /// 记录一次已完成的工具调用：把记录追加到 assistant 节点，并写入事实事件。
    ///
    /// 失败：session 或消息不存在。
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
        max_event_id: snapshot.max_event_id,
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

fn find_node(messages: &[Message], id: MessageId) -> Option<MessageNode> {
    messages
        .iter()
        .find(|message| message.node.id == id)
        .map(|message| message.node.clone())
}

/// 目标子树中最近创建的叶子（id 最大）。
///
/// 消息 id 全局单调递增，因此 id 最大即创建最晚。目标本身是叶子时返回目标。
fn branch_leaf(nodes: &[MessageNode], target: MessageId) -> Option<MessageId> {
    nodes
        .iter()
        .filter(|node| is_descendant(nodes, node.id, target))
        .filter(|node| !nodes.iter().any(|other| other.parent_id == Some(node.id)))
        .map(|node| node.id)
        .max()
}

fn is_descendant(nodes: &[MessageNode], id: MessageId, ancestor: MessageId) -> bool {
    let mut current = Some(id);
    while let Some(node_id) = current {
        if node_id == ancestor {
            return true;
        }
        current = nodes
            .iter()
            .find(|node| node.id == node_id)
            .and_then(|node| node.parent_id);
    }
    false
}
