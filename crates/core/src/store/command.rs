//! 写命令入口：加锁、轻量快照、session 视图推导、提交与发布。
//! 视图的推导本身在 `Changeset::session_view`，入口是它唯一的调用者；
//! 各对象的写命令在 `session`、`queue`、`run`、`branch`、`compaction`、`workspace`、`host`；
//! 读方法留在同一门面，不加锁。

use crate::change::{Change, Changeset};
use crate::error::StoreError;
use crate::id::SessionId;
use crate::session::{SessionMetadata, SessionRecord, SessionView};
use crate::storage::SessionHead;

use super::Store;

/// session 写命令的上下文。闭包在同一把写入线内直接读存储，入口不预先加载消息树。
///
/// 闭包内不得再调用命令入口：写入线不可重入。
pub struct SessionCommand<'a> {
    session: SessionRecord,
    initial: SessionRecord,
    base: SessionView,
    changes: &'a mut Changeset,
    now: i64,
}

impl SessionCommand<'_> {
    fn new(head: SessionHead, changes: &mut Changeset, now: i64) -> SessionCommand<'_> {
        SessionCommand {
            initial: head.session.clone(),
            session: head.session,
            base: head.view,
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

    /// 快照读取时刻的队列长度，不含本次变更。提交后的长度见 `finalize` 推导的视图。
    #[must_use]
    pub fn queued_before(&self) -> u32 {
        self.base.queue_length
    }

    /// 快照读取时刻是否有 running 的 run，不含本次变更。
    #[must_use]
    pub fn busy_before(&self) -> bool {
        self.base.busy
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

    fn deletes_session(&self) -> bool {
        self.changes
            .operations()
            .iter()
            .any(|change| matches!(change, Change::DeleteSession(id) if id == &self.session.id))
    }

    /// 收尾：推导视图、必要时登记 `update_session`，返回提交后的视图。
    ///
    /// 队列长度与 `busy` 取自 `Changeset::session_view`，与事件信封用的是同一次推导。
    /// 记录有变化时 `updated_at` 取本次的 `now`。删除 session 的命令不写 session 行，
    /// 视图为空。
    ///
    /// # Errors
    ///
    /// - `StoreError::Invariant`：推导出的队列长度超出 `u32` 或减到零以下。
    fn finalize(&mut self) -> Result<Option<SessionMetadata>, StoreError> {
        if self.deletes_session() {
            return Ok(None);
        }
        let view = self.changes.session_view(&self.session.id, self.base)?;
        if self.session != self.initial {
            self.session.updated_at = self.now;
            self.changes.update_session(self.session.clone());
        }
        Ok(Some(self.session.metadata(view)))
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
    ///
    /// 新 session 没有队列项也没有 run，因此视图由空基线推导。
    pub fn insert_session(&mut self, session: SessionRecord) -> SessionMetadata {
        let view = session.metadata(SessionView::empty());
        self.view = Some(view.clone());
        self.changes.insert_session(session);
        view
    }
}

impl Store {
    /// 在写入线内执行一条 session 写命令。
    ///
    /// 入口流程：加锁 → 读轻量快照 → 闭包 → 推导 session 视图 → 比较记录并登记
    /// `update_session` → 提交 → 成功后发布 → 返回闭包的值。
    ///
    /// 需要提交后的视图时用 [`Store::command_view`]。
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
        Ok(self.command_view(id, f).await?.0)
    }

    /// 与 [`Store::command`] 相同，但同时返回提交后的 session 视图。
    ///
    /// 视图只能在这里取：它由本次提交的全部变更推导，命令体拿不到部分推导结果。
    /// 删除 session 的命令返回 `None`。
    ///
    /// # Errors
    ///
    /// - `StoreError::SessionNotFound`：session 不存在。
    /// - 闭包返回的错误原样传出；出错时不提交，也不发布事件。
    pub async fn command_view<T>(
        &self,
        id: &SessionId,
        f: impl AsyncFnOnce(&mut SessionCommand<'_>) -> Result<T, StoreError>,
    ) -> Result<(T, Option<SessionMetadata>), StoreError> {
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
        Ok(self.run_session(id, true, f).await?.0)
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
    ) -> Result<(T, Option<SessionMetadata>), StoreError> {
        let _line = self.line.lock().await;
        let head = self
            .storage
            .session_head(id)
            .await?
            .ok_or_else(|| StoreError::SessionNotFound(id.clone()))?;
        if idle {
            if head.view.queue_length > 0 {
                return Err(StoreError::QueueNotEmpty(id.clone()));
            }
            if head.view.busy {
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
        Ok((value, view))
    }
}

fn now_millis() -> i64 {
    jiff::Timestamp::now().as_millisecond()
}
