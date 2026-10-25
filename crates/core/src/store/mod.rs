//! 领域内核的门面：打开数据库、元数据快照、事件回放、提交与读方法转发，
//! 以及各写命令共用的节点入树辅助。
//! 写命令入口在 `command`；按对象的写命令在同级各子模块；存储后端在 `crate::storage`。

mod branch;
mod command;
mod compaction;
mod node;
mod queue;
mod run;
mod session;
mod workspace;

pub use self::command::{Command, SessionCommand};

use std::path::Path;

use tokio::sync::Mutex;

use crate::change::Changeset;
use crate::error::StoreError;
use crate::event::{CommittedEvent, FactEvent};
use crate::id::{MessageId, WorkspacePath};
use crate::message::{Message, MessageNode, MessageRole, MessageSkeleton};
use crate::run::RunRecord;
use crate::session::{SessionMetadata, SessionTree};
use crate::storage::sqlite::SqliteStorage;
use crate::storage::{SessionSnapshot, Snapshot};
use crate::stream::{EventBus, StreamEvent};

/// 一次 run 开始时可用的上下文。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunStart {
    pub run: RunRecord,
    pub workspace: WorkspacePath,
}

/// 领域内核：唯一写入线 + 持久化边界。
///
/// 所有写入经 [`Store::command`]、[`Store::command_idle`] 与 [`Store::command_root`]
/// 串行化，读-改-写之间不会被其它写入插入。每次写入以一个 [`Changeset`] 在一次存储
/// 事务内生效；事务成功前不发布任何事实事件。
pub struct Store {
    storage: SqliteStorage,
    line: Mutex<()>,
    bus: EventBus,
}

impl Store {
    /// 打开 SQLite 文件并应用迁移。父目录不存在时自动建立。
    ///
    /// # Errors
    ///
    /// - `StoreError::DatabaseDirectory`：数据库父目录无法建立。
    /// - `StoreError::Database`：数据库无法打开或连接参数被拒绝。
    /// - `StoreError::Migration`：迁移执行失败。
    ///
    /// 副作用：创建数据库目录、数据库文件与表。
    pub async fn open(path: &Path) -> Result<Self, StoreError> {
        tracing::debug!(database = %path.display(), "打开 SQLite");
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
    /// # Errors
    ///
    /// - `StoreError::Database`：数据库读取失败。
    /// - `StoreError::CorruptedData`：存储值不符合领域约束。
    pub async fn bootstrap(&self) -> Result<Snapshot, StoreError> {
        self.storage.snapshot().await
    }

    /// 从 `after` 游标之后回放事实事件。
    ///
    /// # Errors
    ///
    /// - `StoreError::Database`：数据库读取失败。
    /// - `StoreError::Serialization`：事实事件反序列化失败。
    pub async fn events_after(&self, after: i64) -> Result<Vec<CommittedEvent>, StoreError> {
        self.storage.events_after(after).await
    }

    /// 提交一次变更；事务成功后才发布事实事件。
    ///
    /// `session` 是本次命令推导出的 session 视图，落在每条事件行的信封上。
    async fn commit(
        &self,
        changes: &Changeset,
        session: Option<&SessionMetadata>,
    ) -> Result<Vec<CommittedEvent>, StoreError> {
        let committed = self.storage.apply(changes, session).await?;
        for event in &committed {
            self.bus.publish(StreamEvent::Fact {
                id: event.id,
                session_id: event.event.session_id().cloned(),
                session: event.session.clone(),
                timestamp: event.created_at,
                event: event.event.clone(),
            });
        }
        Ok(committed)
    }
}

/// 登记一条消息入树：写节点、移动 active leaf，并写入 `message_appended` 与 `active_leaf_changed`。
///
/// 入树的四类消息（assistant、工具、压缩、用户插话）共用它，事件顺序因此一致。
fn append_node(command: &mut SessionCommand<'_>, node: &MessageNode) {
    let session = command.session().id.clone();
    let now = command.now();
    command.changes().insert_message(Message {
        session: session.clone(),
        node: node.clone(),
    });
    command.session_mut().active_leaf = Some(node.id);
    command.changes().append_event(
        FactEvent::MessageAppended {
            session_id: session.clone(),
            message: node.skeleton(),
        },
        now,
    );
    command.changes().append_event(
        FactEvent::ActiveLeafChanged {
            session_id: session,
            active_leaf: Some(node.id),
        },
        now,
    );
}

fn snapshot_into_tree(snapshot: SessionSnapshot) -> SessionTree {
    SessionTree {
        session: snapshot
            .head
            .session
            .metadata(snapshot.head.queue_length, snapshot.head.busy),
        max_event_id: snapshot.max_event_id,
        active_leaf: snapshot.head.session.active_leaf,
        nodes: snapshot.messages,
        queue: snapshot.queue,
        runs: snapshot.runs,
    }
}

fn find_node(messages: &[MessageSkeleton], id: MessageId) -> Option<MessageSkeleton> {
    messages.iter().find(|node| node.id == id).cloned()
}

/// 从给定节点沿父链向上找到最近一条用户消息。
fn user_ancestor(nodes: &[MessageSkeleton], start: MessageId) -> Option<MessageId> {
    let mut current = Some(start);
    while let Some(id) = current {
        let node = nodes.iter().find(|node| node.id == id)?;
        if node.role == MessageRole::User {
            return Some(node.id);
        }
        current = node.parent_id;
    }
    None
}

/// 目标子树中最近创建的叶子（id 最大）。
///
/// 消息 id 全局单调递增，因此 id 最大即创建最晚。目标本身是叶子时返回目标。
fn branch_leaf(nodes: &[MessageSkeleton], target: MessageId) -> Option<MessageId> {
    nodes
        .iter()
        .filter(|node| is_descendant(nodes, node.id, target))
        .filter(|node| !nodes.iter().any(|other| other.parent_id == Some(node.id)))
        .map(|node| node.id)
        .max()
}

fn is_descendant(nodes: &[MessageSkeleton], id: MessageId, ancestor: MessageId) -> bool {
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
