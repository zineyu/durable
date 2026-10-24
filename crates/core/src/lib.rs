//! 领域内核：领域类型、唯一写入线与 SQLite 存储。
//! 进程启动、HTTP 协议与客户端在 `crates/agent`、`crates/hub` 与 `client`。

mod change;
mod error;
mod event;
mod id;
mod message;
mod node;
mod queue;
mod run;
mod session;
mod storage;
mod store;
mod stream;
mod tool;
mod workspace;

pub use change::Changeset;
pub use error::StoreError;
pub use event::{CommittedEvent, FactEvent};
pub use id::{
    DEFAULT_SESSION_TITLE, MessageId, MessageIdError, QueueItemId, QueueItemIdError, RunId,
    RunIdError, SessionId, SessionIdError, SessionTitle, SessionTitleError, WorkspacePath,
    WorkspacePathError,
};
pub use message::{
    ContentKind, ContentKindError, ContentPart, Message, MessageBody, MessageNode, MessageRole,
    MessageRoleError, MessageSkeleton, MessageText, MessageTextError, PartSkeleton, PartState,
    PartStateError, ProviderMeta,
};
pub use node::{NodeMetadata, NodeName, NodeNameError};
pub use queue::{QueueItem, SubmitMode};
pub use run::{RunFailure, RunFailureCode, RunRecord, RunStatus, RunStatusError};
pub use session::{SessionMetadata, SessionRecord, SessionTree};
pub use storage::Snapshot;
pub use store::{RunStart, Store};
pub use stream::{DeltaKind, EventBus, ResyncReason, StreamEvent};
pub use tool::{ToolErrorCode, ToolOutcome, ToolResultState};
pub use workspace::WorkspaceMetadata;
