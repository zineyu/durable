//! 一次存储事务内的变更清单：按顺序累积行操作与事实事件，交给存储层落库。
//! session 视图的推导也在这里（`Changeset::session_view`）；行写入在 `storage/sqlite/apply`，
//! 领域类型在同级各对象模块。

use crate::error::StoreError;
use crate::event::FactEvent;
use crate::host::{HostMetadata, HostName};
use crate::id::{MessageId, QueueItemId, RunId, SessionId, SessionTitle, WorkspacePath};
use crate::message::{ContentPart, Message};
use crate::queue::QueueItem;
use crate::run::{RunFailure, RunRecord, RunStatus};
use crate::session::{SessionRecord, SessionView};
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

    pub fn insert_host(&mut self, host: HostMetadata) {
        self.push(Change::InsertHost(host));
    }

    /// 写入 host 的可变字段：名称、目标、启用标志与更新时间。
    pub fn update_host(&mut self, host: HostMetadata) {
        self.push(Change::UpdateHost(host));
    }

    /// 改名只更改主键；`ON UPDATE CASCADE` 让 workspace 与 session 跟着更换。
    pub fn rename_host(&mut self, from: HostName, to: HostName) {
        self.push(Change::RenameHost { from, to });
    }

    pub fn delete_host(&mut self, name: HostName) {
        self.push(Change::DeleteHost(name));
    }

    /// 记下或清空 host 上已安装的 executor 构建 id。它不是领域事实，不写事件。
    pub fn set_host_installed_build(&mut self, name: HostName, build: Option<String>) {
        self.push(Change::SetHostInstalledBuild { name, build });
    }

    pub fn insert_workspace(&mut self, workspace: WorkspaceMetadata) {
        self.push(Change::InsertWorkspace(workspace));
    }

    /// 移除 workspace。identity 是 host 与路径这一对。
    pub fn delete_workspace(&mut self, host: HostName, path: WorkspacePath) {
        self.push(Change::DeleteWorkspace { host, path });
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

    /// 改写 run 的流式进度。不推进 run 的更新时间，也不影响 session 视图。
    pub fn update_run_progress(&mut self, id: RunId, progress: Option<&str>) {
        self.push(Change::UpdateRunProgress {
            id,
            progress: progress.map(ToOwned::to_owned),
        });
    }

    pub fn append_event(&mut self, event: FactEvent, created_at: i64) {
        self.push(Change::AppendEvent { event, created_at });
    }

    /// 由视图基线与本次登记的全部变更求出提交后的视图。
    ///
    /// 影响视图的变更只有队列项与 run 两类。`match` 逐个列出变体、不带兜底分支：新增变更
    /// 种类时编译器要求作者表态。队列项只统计指向 `session` 的变更，因此一条命令只能写
    /// 一个 session。
    ///
    /// # Errors
    ///
    /// - `StoreError::Invariant`：推导出的队列长度超出 `u32`，或减到负数。此时基线与变更
    ///   清单不一致，提交应当失败。
    pub(crate) fn session_view(
        &self,
        session: &SessionId,
        base: SessionView,
    ) -> Result<SessionView, StoreError> {
        let mut view = base;
        for change in &self.operations {
            match change {
                Change::InsertQueueItem { session: owner, .. } => {
                    if owner == session {
                        view.queue_length = view.queue_length.checked_add(1).ok_or_else(|| {
                            StoreError::Invariant("推导出的队列长度超出 u32".to_owned())
                        })?;
                    }
                }
                Change::DeleteQueueItem { session: owner, .. } => {
                    if owner == session {
                        view.queue_length = view.queue_length.checked_sub(1).ok_or_else(|| {
                            StoreError::Invariant("推导出的队列长度减到零以下".to_owned())
                        })?;
                    }
                }
                Change::InsertRun(run) => view.busy = run.status == RunStatus::Running,
                Change::UpdateRun { status, .. } => view.busy = *status == RunStatus::Running,
                Change::ClearPendingSteer { .. }
                | Change::InsertMessage(_)
                | Change::ReplaceMessageParts { .. }
                | Change::BindMessageRun { .. }
                | Change::RebindMessageParent { .. }
                | Change::UpdateRunProgress { .. }
                | Change::InsertSession(_)
                | Change::UpdateSession { .. }
                | Change::DeleteSession(_)
                | Change::InsertWorkspace(_)
                | Change::DeleteWorkspace { .. }
                | Change::InsertHost(_)
                | Change::UpdateHost(_)
                | Change::RenameHost { .. }
                | Change::DeleteHost(_)
                | Change::SetHostInstalledBuild { .. }
                | Change::AppendEvent { .. } => {}
            }
        }
        Ok(view)
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
    DeleteWorkspace {
        host: HostName,
        path: WorkspacePath,
    },
    InsertHost(HostMetadata),
    UpdateHost(HostMetadata),
    RenameHost {
        from: HostName,
        to: HostName,
    },
    DeleteHost(HostName),
    SetHostInstalledBuild {
        name: HostName,
        build: Option<String>,
    },
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
    UpdateRunProgress {
        id: RunId,
        progress: Option<String>,
    },
    AppendEvent {
        event: FactEvent,
        created_at: i64,
    },
}
