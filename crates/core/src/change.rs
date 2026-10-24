//! 一次存储事务内的变更清单：按顺序累积行操作与事实事件，交给存储层落库。
//! 行写入在 `storage/sqlite/apply`，领域类型在同级各对象模块。

use crate::event::FactEvent;
use crate::id::{MessageId, QueueItemId, RunId, SessionId, SessionTitle, WorkspacePath};
use crate::message::{ContentPart, Message};
use crate::node::{NodeMetadata, NodeName};
use crate::queue::QueueItem;
use crate::run::{RunFailure, RunRecord, RunStatus};
use crate::session::SessionRecord;
use crate::workspace::WorkspaceMetadata;

/// 一次原子提交的全部变更。字段私有，只能经下面的方法逐个登记，保证顺序与意图明确。
#[derive(Debug, Default)]
pub struct Changeset {
    operations: Vec<Change>,
}

impl Changeset {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_workspace(&mut self, workspace: WorkspaceMetadata) {
        self.push(Change::InsertWorkspace(workspace));
    }

    pub fn delete_workspace(&mut self, path: WorkspacePath) {
        self.push(Change::DeleteWorkspace(path));
    }

    pub fn insert_session(&mut self, session: SessionRecord) {
        self.push(Change::InsertSession(session));
    }

    /// 写入 session 的全部可变字段：标题、标题是否自动、更新时间与 active leaf。
    pub fn update_session(
        &mut self,
        id: SessionId,
        title: SessionTitle,
        title_auto: bool,
        updated_at: i64,
        active_leaf: Option<MessageId>,
    ) {
        self.push(Change::UpdateSession {
            id,
            title,
            title_auto,
            updated_at,
            active_leaf,
        });
    }

    pub fn delete_session(&mut self, id: SessionId) {
        self.push(Change::DeleteSession(id));
    }

    pub fn insert_message(&mut self, message: Message) {
        self.push(Change::InsertMessage(message));
    }

    pub fn insert_queue_item(&mut self, session: SessionId, item: QueueItem) {
        self.push(Change::InsertQueueItem { session, item });
    }

    pub fn insert_node(&mut self, node: NodeMetadata) {
        self.push(Change::InsertNode(node));
    }

    pub fn delete_node(&mut self, name: NodeName) {
        self.push(Change::DeleteNode(name));
    }

    pub fn delete_queue_item(&mut self, session: SessionId, item: QueueItemId) {
        self.push(Change::DeleteQueueItem { session, item });
    }

    /// 整体替换一条已存在消息的块。用于工具节点从执行中改为结局。
    pub fn replace_message_parts(&mut self, id: MessageId, parts: Vec<ContentPart>) {
        self.push(Change::ReplaceMessageParts { id, parts });
    }

    /// 把已存在的用户节点绑定到 run。用于编辑产生的节点在 run 开始时补上 run 关联。
    pub fn bind_message_run(&mut self, id: MessageId, run_id: RunId) {
        self.push(Change::BindMessageRun { id, run_id });
    }

    /// 改写消息的父节点。用于插话接入链尾时从提交时的叶子挪到当前叶子。
    pub fn rebind_message_parent(&mut self, id: MessageId, parent_id: Option<MessageId>) {
        self.push(Change::RebindMessageParent { id, parent_id });
    }

    /// 把待插入项降级为普通排队项。
    pub fn clear_pending_steer(&mut self, session: SessionId, item: QueueItemId) {
        self.push(Change::ClearPendingSteer { session, item });
    }

    pub fn insert_run(&mut self, run: RunRecord) {
        self.push(Change::InsertRun(run));
    }

    pub fn update_run(
        &mut self,
        id: RunId,
        status: RunStatus,
        failure: Option<RunFailure>,
        updated_at: i64,
    ) {
        self.push(Change::UpdateRun {
            id,
            status,
            failure,
            updated_at,
        });
    }

    pub fn append_event(&mut self, event: FactEvent, created_at: i64) {
        self.push(Change::AppendEvent { event, created_at });
    }

    fn push(&mut self, change: Change) {
        self.operations.push(change);
    }

    pub(crate) fn operations(&self) -> &[Change] {
        &self.operations
    }
}

#[derive(Debug)]
pub(crate) enum Change {
    InsertWorkspace(WorkspaceMetadata),
    DeleteWorkspace(WorkspacePath),
    InsertSession(SessionRecord),
    InsertNode(NodeMetadata),
    DeleteNode(NodeName),
    UpdateSession {
        id: SessionId,
        title: SessionTitle,
        title_auto: bool,
        updated_at: i64,
        active_leaf: Option<MessageId>,
    },
    DeleteSession(SessionId),
    InsertMessage(Message),
    InsertQueueItem {
        session: SessionId,
        item: QueueItem,
    },
    DeleteQueueItem {
        session: SessionId,
        item: QueueItemId,
    },
    ReplaceMessageParts {
        id: MessageId,
        parts: Vec<ContentPart>,
    },
    BindMessageRun {
        id: MessageId,
        run_id: RunId,
    },
    RebindMessageParent {
        id: MessageId,
        parent_id: Option<MessageId>,
    },
    ClearPendingSteer {
        session: SessionId,
        item: QueueItemId,
    },
    InsertRun(RunRecord),
    UpdateRun {
        id: RunId,
        status: RunStatus,
        failure: Option<RunFailure>,
        updated_at: i64,
    },
    AppendEvent {
        event: FactEvent,
        created_at: i64,
    },
}
