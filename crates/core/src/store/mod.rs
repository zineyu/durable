//! 领域内核的门面：打开数据库、元数据快照、事件回放与提交入口。
//! 写入命令按对象拆在同级子模块；存储后端在 `crate::storage`。

mod branch;
mod node;
mod queue;
mod run;
mod session;
mod workspace;

use std::path::Path;

use tokio::sync::Mutex;

use crate::change::Changeset;
use crate::error::StoreError;
use crate::event::CommittedEvent;
use crate::id::{MessageId, WorkspacePath};
use crate::message::{Message, MessageNode};
use crate::run::RunRecord;
use crate::session::SessionTree;
use crate::storage::sqlite::SqliteStorage;
use crate::storage::{SessionSnapshot, Snapshot};
use crate::stream::{EventBus, StreamEvent};

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

fn snapshot_into_tree(snapshot: SessionSnapshot) -> SessionTree {
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
