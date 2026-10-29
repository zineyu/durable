//! 写命令入口：加锁、轻量快照、session 视图推导、提交与发布。
//! 各对象的写命令在 `session`、`queue`、`run`、`branch`、`compaction`、`workspace`、`host`；
//! 读方法留在同一门面，不加锁。

use crate::change::{Change, Changeset};
use crate::error::StoreError;
use crate::id::SessionId;
use crate::run::RunStatus;
use crate::session::{SessionMetadata, SessionRecord};
use crate::storage::SessionHead;

use super::Store;

/// session 写命令的上下文。闭包在同一把写入线内直接读存储，入口不预先加载消息树。
///
/// 闭包内不得再调用命令入口：写入线不可重入。
pub struct SessionCommand<'a> {
    session: SessionRecord,
    initial: SessionRecord,
    queue_length: u32,
    busy: bool,
    changes: &'a mut Changeset,
    now: i64,
}

impl SessionCommand<'_> {
    fn new(head: SessionHead, changes: &mut Changeset, now: i64) -> SessionCommand<'_> {
        SessionCommand {
            initial: head.session.clone(),
            session: head.session,
            queue_length: head.queue_length,
            busy: head.busy,
            changes,
            now,
        }
    }

    /// 当前 session 记录。命令改写它的字段，入口负责写库。
    #[must_use]
    pub fn session(&self) -> &SessionRecord {
        &self.session
    }

    /// 可变借用的 session 记录。命令只改记录，不登记 `update_session`。
    pub fn session_mut(&mut self) -> &mut SessionRecord {
        &mut self.session
    }

    /// 快照读取时刻的队列长度，不含本次变更。
    #[must_use]
    pub fn queue_length(&self) -> u32 {
        self.queue_length
    }

    /// 快照读取时刻是否有 running 的 run，不含本次变更。
    #[must_use]
    pub fn busy(&self) -> bool {
        self.busy
    }

    /// 本次变更清单。
    pub fn changes(&mut self) -> &mut Changeset {
        self.changes
    }

    /// 本次命令的时间戳。入口保证一次命令只用同一个时间戳。
    #[must_use]
    pub fn now(&self) -> i64 {
        self.now
    }

    /// 提交后的 session 视图。
    ///
    /// 队列长度与 `busy` 由本次变更推导；记录有变化时 `updated_at` 取本次的 `now`。
    /// 入口用同一个推导填写事件信封，因此需要返回视图的命令可以直接返回它。
    ///
    /// 影响视图的变更只有 `InsertQueueItem`、`DeleteQueueItem`、`InsertRun` 与 `UpdateRun`；
    /// 新增会改变队列长度或 run 状态的变更种类时，必须同步扩展这里。一条命令只写一个
    /// session，因此推导不按 session 过滤。
    ///
    /// # Errors
    ///
    /// - `StoreError::Invariant`：推导出的队列长度超出 `u32`。
    pub(crate) fn view(&self) -> Result<SessionMetadata, StoreError> {
        let mut session = self.session.clone();
        if session != self.initial {
            session.updated_at = self.now;
        }
        Ok(session.metadata(self.queue_length_after()?, self.busy_after()))
    }

    fn queue_length_after(&self) -> Result<u32, StoreError> {
        let mut length = i64::from(self.queue_length);
        for change in self.changes.operations() {
            length += match change {
                Change::InsertQueueItem { session, .. } if session == &self.session.id => 1,
                Change::DeleteQueueItem { session, .. } if session == &self.session.id => -1,
                _ => 0,
            };
        }
        u32::try_from(length)
            .map_err(|_| StoreError::Invariant(format!("推导出的队列长度超出 u32: {length}")))
    }

    fn busy_after(&self) -> bool {
        let mut busy = self.busy;
        for change in self.changes.operations() {
            match change {
                Change::InsertRun(run) => busy = run.status == RunStatus::Running,
                Change::UpdateRun { status, .. } => busy = *status == RunStatus::Running,
                _ => {}
            }
        }
        busy
    }

    fn deletes_session(&self) -> bool {
        self.changes
            .operations()
            .iter()
            .any(|change| matches!(change, Change::DeleteSession(id) if id == &self.session.id))
    }

    /// 收尾：记录有变化时补 `updated_at` 并登记 `update_session`，再返回提交后的视图。
    /// 删除 session 的命令不写 session 行，视图为空。
    fn finalize(&mut self) -> Result<Option<SessionMetadata>, StoreError> {
        if self.deletes_session() {
            return Ok(None);
        }
        let view = self.view()?;
        if self.session != self.initial {
            self.session.updated_at = self.now;
            self.changes.update_session(
                self.session.id.clone(),
                self.session.title.clone(),
                self.session.title_auto,
                self.now,
                self.session.active_leaf,
            );
        }
        Ok(Some(view))
    }
}

/// 不读 session 快照的写命令的上下文：host、workspace 与新建 session。
pub struct Command<'a> {
    changes: &'a mut Changeset,
    now: i64,
    /// 本次命令产生的 session 视图。只有新建 session 的命令会产生它。
    view: Option<SessionMetadata>,
}

impl Command<'_> {
    /// 本次变更清单。
    pub fn changes(&mut self) -> &mut Changeset {
        self.changes
    }

    /// 本次命令的时间戳。入口保证一次命令只用同一个时间戳。
    #[must_use]
    pub fn now(&self) -> i64 {
        self.now
    }

    /// 登记新 session 的插入，并返回它提交后的视图。
    pub fn insert_session(&mut self, session: SessionRecord) -> SessionMetadata {
        let view = created_view(&session);
        self.view = Some(view.clone());
        self.changes.insert_session(session);
        view
    }
}

/// 新 session 的视图：队列为空，没有 run。入口据此填写 `session_created` 的信封。
fn created_view(session: &SessionRecord) -> SessionMetadata {
    session.metadata(0, false)
}

impl Store {
    /// 在写入线内执行一条 session 写命令。
    ///
    /// 入口流程：加锁 → 读轻量快照 → 闭包 → 推导 session 视图 → 比较记录并登记
    /// `update_session` → 提交 → 成功后发布 → 返回闭包的值。
    ///
    /// # Errors
    ///
    /// - `StoreError::SessionNotFound`：session 不存在。
    /// - 闭包返回的错误原样传出；出错时不提交，也不发布事件。
    pub async fn command<T>(
        &self,
        id: &SessionId,
        f: impl AsyncFnOnce(&mut SessionCommand<'_>) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        self.run_session(id, false, f).await
    }

    /// 与 [`Store::command`] 相同，但先要求 session 空闲。
    ///
    /// 队列非空优先于 run 进行中报告。
    ///
    /// # Errors
    ///
    /// - `StoreError::SessionNotFound`：session 不存在。
    /// - `StoreError::QueueNotEmpty`：session 队列非空。
    /// - `StoreError::RunInProgress`：session 有 run 进行中。
    /// - 闭包返回的错误原样传出。
    pub async fn command_idle<T>(
        &self,
        id: &SessionId,
        f: impl AsyncFnOnce(&mut SessionCommand<'_>) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        self.run_session(id, true, f).await
    }

    /// 在写入线内执行一条不读 session 快照的写命令。
    ///
    /// # Errors
    ///
    /// - 闭包返回的错误原样传出；出错时不提交，也不发布事件。
    pub async fn command_root<T>(
        &self,
        f: impl AsyncFnOnce(&mut Command<'_>) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        let _line = self.line.lock().await;
        let mut changes = Changeset::new();
        let (value, view) = {
            let mut command = Command {
                changes: &mut changes,
                now: now_millis(),
                view: None,
            };
            let value = f(&mut command).await?;
            (value, command.view.clone())
        };
        self.commit(&changes, view.as_ref()).await?;
        Ok(value)
    }

    async fn run_session<T>(
        &self,
        id: &SessionId,
        idle: bool,
        f: impl AsyncFnOnce(&mut SessionCommand<'_>) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        let _line = self.line.lock().await;
        let head = self
            .storage
            .session_head(id)
            .await?
            .ok_or_else(|| StoreError::SessionNotFound(id.clone()))?;
        if idle {
            if head.queue_length > 0 {
                return Err(StoreError::QueueNotEmpty(id.clone()));
            }
            if head.busy {
                return Err(StoreError::RunInProgress(id.clone()));
            }
        }
        let mut changes = Changeset::new();
        let (value, view) = {
            let mut command = SessionCommand::new(head, &mut changes, now_millis());
            let value = f(&mut command).await?;
            let view = command.finalize()?;
            (value, view)
        };
        self.commit(&changes, view.as_ref()).await?;
        Ok(value)
    }
}

fn now_millis() -> i64 {
    jiff::Timestamp::now().as_millisecond()
}
