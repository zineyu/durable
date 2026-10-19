use crate::event::FactEvent;
use crate::id::{MessageId, SessionId, SessionTitle, WorkspacePath};
use crate::message::Message;
use crate::queue::QueueItem;
use crate::session::SessionRecord;
use crate::workspace::WorkspaceMetadata;

/// 一次原子提交的全部变更。字段私有，只能经下面的方法逐个登记，保证顺序与意图明确。
#[derive(Debug, Default)]
pub struct Changeset {
    operations: Vec<Change>,
}

impl Changeset {
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
    AppendEvent {
        event: FactEvent,
        created_at: i64,
    },
}
