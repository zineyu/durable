pub mod sqlite;

use std::future::Future;

use crate::change::Changeset;
use crate::error::StoreError;
use crate::id::{MessageId, SessionId, WorkspacePath};
use crate::message::Message;
use crate::session::{SessionMetadata, SessionRecord};
use crate::workspace::WorkspaceMetadata;

/// bootstrap 所需的元数据快照。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub workspaces: Vec<WorkspaceMetadata>,
    pub sessions: Vec<SessionMetadata>,
    pub max_event_id: i64,
}

/// 持久化边界。所有写入经 [`Storage::apply`] 以单个事务生效。
pub trait Storage: Send + Sync {
    fn snapshot(&self) -> impl Future<Output = Result<Snapshot, StoreError>> + Send;

    fn workspace(
        &self,
        path: &WorkspacePath,
    ) -> impl Future<Output = Result<Option<WorkspaceMetadata>, StoreError>> + Send;

    fn session(
        &self,
        id: &SessionId,
    ) -> impl Future<Output = Result<Option<SessionRecord>, StoreError>> + Send;

    fn session_count(
        &self,
        workspace: &WorkspacePath,
    ) -> impl Future<Output = Result<i64, StoreError>> + Send;

    fn messages(
        &self,
        session: &SessionId,
    ) -> impl Future<Output = Result<Vec<Message>, StoreError>> + Send;

    fn apply(&self, changes: &Changeset) -> impl Future<Output = Result<(), StoreError>> + Send;

    fn mint_message_id(&self) -> impl Future<Output = Result<MessageId, StoreError>> + Send;
}
