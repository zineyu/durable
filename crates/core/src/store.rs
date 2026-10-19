use std::path::Path;

use tokio::sync::Mutex;

use crate::change::Changeset;
use crate::error::StoreError;
use crate::event::FactEvent;
use crate::id::{SessionId, SessionTitle, WorkspacePath};
use crate::message::{Message, MessageNode, MessageRole, MessageText};
use crate::session::{SessionMetadata, SessionRecord, SessionTree};
use crate::storage::sqlite::SqliteStorage;
use crate::storage::{Snapshot, Storage};
use crate::workspace::WorkspaceMetadata;

/// 领域内核：唯一写入线 + 持久化边界。
///
/// 所有写入经 [`Store::lock_line`] 串行化，读-改-写之间不会被其它写入插入。每次写入以
/// 一个 [`Changeset`] 在一次存储事务内生效；事务成功前不发布任何事实事件。
pub struct Store<S: Storage> {
    storage: S,
    line: Mutex<()>,
}

impl Store<SqliteStorage> {
    /// 打开 SQLite 文件并应用迁移。
    ///
    /// 失败：数据库无法打开、连接参数被拒绝或迁移失败。
    /// 副作用：创建数据库文件与表。
    pub async fn open(path: &Path) -> Result<Self, StoreError> {
        Ok(Self::new(SqliteStorage::open(path).await?))
    }
}

impl<S: Storage> Store<S> {
    pub fn new(storage: S) -> Self {
        Self {
            storage,
            line: Mutex::new(()),
        }
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
        self.register_workspace_locked(path).await
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
            self.register_workspace_locked(path).await?;
        }
        Ok(())
    }

    async fn register_workspace_locked(
        &self,
        path: &WorkspacePath,
    ) -> Result<WorkspaceMetadata, StoreError> {
        if self.storage.workspace(path).await?.is_some() {
            return Err(StoreError::WorkspaceDuplicate(path.clone()));
        }
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
        self.storage.apply(&changes).await?;
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
        self.storage.apply(&changes).await
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
            created_at,
            updated_at: created_at,
            active_leaf: None,
        };
        let mut changes = Changeset::new();
        changes.insert_session(session.clone());
        changes.append_event(
            FactEvent::SessionCreated {
                session: session.metadata(),
            },
            created_at,
        );
        self.storage.apply(&changes).await?;
        Ok(session.metadata())
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
        session.updated_at = now_millis();
        let mut changes = Changeset::new();
        changes.update_session(session.clone());
        changes.append_event(
            FactEvent::SessionRenamed {
                session: session.metadata(),
            },
            session.updated_at,
        );
        self.storage.apply(&changes).await?;
        Ok(session.metadata())
    }

    /// 删除 session。
    ///
    /// 输入：session 身份。
    /// 失败：session 不存在。
    /// 副作用：删除该 session 的消息树与历史事件，再写入 `session_deleted` 事实事件。
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
        self.storage.apply(&changes).await
    }

    /// 读取单个 session 的消息树。
    ///
    /// 失败：session 不存在。
    pub async fn session_tree(&self, id: &SessionId) -> Result<SessionTree, StoreError> {
        let session = self.find_session(id).await?;
        let nodes = self
            .storage
            .messages(id)
            .await?
            .into_iter()
            .map(|message| message.node)
            .collect();
        Ok(SessionTree {
            session: session.metadata(),
            active_leaf: session.active_leaf,
            nodes,
        })
    }

    /// 将一条用户消息追加为当前 active leaf 的子节点。
    ///
    /// 输入：session 身份与已校验的消息正文。
    /// 失败：session 不存在。
    /// 副作用：写入消息行、移动 active leaf、更新 `updated_at`，写入 `message_appended` 事实事件。
    pub async fn admit_message(
        &self,
        id: &SessionId,
        text: MessageText,
    ) -> Result<MessageNode, StoreError> {
        let _line = self.line.lock().await;
        let mut session = self.find_session(id).await?;
        let message_id = self.storage.mint_message_id().await?;
        let created_at = now_millis();
        let node = MessageNode {
            id: message_id,
            parent_id: session.active_leaf,
            role: MessageRole::User,
            text,
            created_at,
        };
        session.updated_at = created_at;
        session.active_leaf = Some(message_id);
        let mut changes = Changeset::new();
        changes.insert_message(Message {
            session: id.clone(),
            node: node.clone(),
        });
        changes.update_session(session.clone());
        changes.set_active_leaf(id.clone(), message_id);
        changes.append_event(
            FactEvent::MessageAppended {
                session: session.metadata(),
                message: node.clone(),
            },
            created_at,
        );
        self.storage.apply(&changes).await?;
        Ok(node)
    }

    async fn find_session(&self, id: &SessionId) -> Result<SessionRecord, StoreError> {
        self.storage
            .session(id)
            .await?
            .ok_or_else(|| StoreError::SessionNotFound(id.clone()))
    }
}

fn now_millis() -> i64 {
    jiff::Timestamp::now().as_millisecond()
}
