//! host 的运行状态：每个 host 一条 link、状态、说明与心跳延迟。
//! host 表与事实在 `durable` 的 `store/host`；link 的帧与关闭状态在 `link`；
//! ssh 的探测与安装在 `link::ssh`；ssh 的后台连接与退避在 `ssh`；对外快照 DTO 在 `protocol`。
//!
//! 规格 user story R-8、R-11、R-12、R-14、R-15、R-16、R-17、R-18、R-35、R-46：
//! 本机 host 启动即自动注册，状态三态显式可见，ssh host 首次连接时自动安装 executor，
//! 断链后按退避重连，同一 host 上的多个 session 共用一条 link。

mod ssh;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use durable::link::WorkspaceCheckError;
use durable::{
    HostKind, HostMetadata, HostName, HostStatus, HostUpdate, Store, StoreError, ToolLimits,
};
use tokio::sync::{Mutex, watch};

use crate::link::{LinkClosed, LinkHandle};
use crate::protocol::HostSnapshot;

/// 心跳间隔。往返毫秒数写进 host 快照的延迟字段。
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);

/// 本机 host 的固定名字。首次启动自动注册。
pub const LOCAL_HOST: &str = "local";

/// 停用 host 的状态说明。
pub(crate) const DISABLED_DETAIL: &str = "host 已停用";

/// 一个 host 的进程内状态。状态本身不落库，只经事实事件对外。
struct HostRuntime {
    status: HostStatus,
    detail: Option<String>,
    latency_ms: Option<u64>,
    link: Option<Arc<LinkHandle>>,
    /// 每次启动 link 递增。关闭通知带上代号，过期的通知直接丢弃。
    generation: u64,
    /// ssh host 的后台连接任务。丢弃它即让任务退出：重连、停用与删除都这样做。
    cancel: Option<watch::Sender<()>>,
}

impl HostRuntime {
    fn disconnected() -> Self {
        Self {
            status: HostStatus::Unreachable,
            detail: None,
            latency_ms: None,
            link: None,
            generation: 0,
            cancel: None,
        }
    }
}

/// host 注册表：host 的运行状态与 link 的唯一持有者。
///
/// 状态迁移只有三条来源：启动或重连时的一次连接、link 关闭通知、停用与更改目标。
/// 每次状态或说明变化都写一条 `host_status_changed` 事实；延迟不写事实。
pub struct HostRegistry {
    store: Arc<Store>,
    executor: PathBuf,
    limits: ToolLimits,
    /// 心跳间隔。生产固定 15 秒；测试用它把一轮心跳压缩到毫秒级。
    interval: Duration,
    hosts: Mutex<HashMap<HostName, HostRuntime>>,
    sequence: AtomicU64,
}

impl HostRegistry {
    /// 建立注册表：确保 `local` host 已登记，并为全部启用的 host 拉起 link。
    ///
    /// # Errors
    ///
    /// - `StoreError::Database`：读取 host 表失败。
    /// - `StoreError::Serialization`：登记 `local` host 时事件序列化失败。
    pub async fn start(
        store: Arc<Store>,
        executor: PathBuf,
        limits: ToolLimits,
    ) -> Result<Arc<Self>, StoreError> {
        Self::start_with_interval(store, executor, limits, HEARTBEAT_INTERVAL).await
    }

    /// 与 [`HostRegistry::start`] 相同，但用心跳间隔替换默认的 15 秒。
    ///
    /// # Errors
    ///
    /// 与 [`HostRegistry::start`] 相同。
    pub async fn start_with_interval(
        store: Arc<Store>,
        executor: PathBuf,
        limits: ToolLimits,
        interval: Duration,
    ) -> Result<Arc<Self>, StoreError> {
        let registry = Arc::new(Self {
            store,
            executor,
            limits,
            interval,
            hosts: Mutex::new(HashMap::new()),
            sequence: AtomicU64::new(1),
        });
        registry.ensure_local().await?;
        registry.connect_all().await;
        registry.spawn_heartbeat();
        Ok(registry)
    }

    /// bootstrap 与命令响应用的快照列表，按注册顺序排列。
    ///
    /// # Errors
    ///
    /// - `StoreError::Database`：读取 host 表失败。
    /// - `StoreError::CorruptedData`：存储值不符合领域约束。
    pub async fn snapshots(&self) -> Result<Vec<HostSnapshot>, StoreError> {
        let hosts = self.store.hosts().await?;
        let runtime = self.hosts.lock().await;
        Ok(hosts
            .into_iter()
            .map(|host| {
                let state = runtime.get(&host.name);
                Self::snapshot_of(host, state)
            })
            .collect())
    }

    /// 单个 host 的快照。
    ///
    /// # Errors
    ///
    /// - `StoreError::Database`：读取 host 表失败。
    /// - `StoreError::CorruptedData`：存储值不符合领域约束。
    pub async fn snapshot(&self, name: &HostName) -> Result<Option<HostSnapshot>, StoreError> {
        let Some(host) = self.store.host(name).await? else {
            return Ok(None);
        };
        let runtime = self.hosts.lock().await;
        Ok(Some(Self::snapshot_of(host, runtime.get(name))))
    }

    /// 注册 host 并立即尝试连接。
    ///
    /// # Errors
    ///
    /// - `StoreError::HostNameTaken`：该名称已注册。
    /// - `StoreError::Database`：数据库读取或写入失败。
    /// - `StoreError::Serialization`：事实事件序列化失败。
    pub async fn register(
        self: &Arc<Self>,
        name: HostName,
        kind: HostKind,
        target: Option<String>,
    ) -> Result<HostSnapshot, StoreError> {
        let host = self.store.register_host(name, kind, target).await?;
        self.connect(&host.name).await;
        Ok(self.snapshot_of_now(host).await)
    }

    /// 更新 host：改名、更换目标、启用或停用，随后重建它的 link。
    ///
    /// 改名后旧名立即成为未知 host；旧 link 随改名关闭。
    ///
    /// # Errors
    ///
    /// - `StoreError::HostNotFound`：当前名不在 host 表中。
    /// - `StoreError::HostNameTaken`：改名后的新名已被使用。
    /// - `StoreError::Database`：数据库读取或写入失败。
    /// - `StoreError::Serialization`：事实事件序列化失败。
    pub async fn update(
        self: &Arc<Self>,
        name: &HostName,
        update: HostUpdate,
    ) -> Result<HostSnapshot, StoreError> {
        let host = self.store.update_host(name, update).await?;
        if &host.name != name {
            self.hosts.lock().await.remove(name);
        }
        self.connect(&host.name).await;
        Ok(self.snapshot_of_now(host).await)
    }

    /// 移除 host：关闭它的 link 并丢弃运行状态。
    ///
    /// # Errors
    ///
    /// - `StoreError::HostNotFound`：该名称不在 host 表中。
    /// - `StoreError::HostNotEmpty`：该 host 仍登记有 workspace。
    /// - `StoreError::Database`：数据库读写失败。
    /// - `StoreError::Serialization`：事实事件序列化失败。
    pub async fn remove(&self, name: &HostName) -> Result<(), StoreError> {
        self.store.remove_host(name).await?;
        self.hosts.lock().await.remove(name);
        Ok(())
    }

    /// 手动重连：关掉当前 link 并按当前行立即重启一条，退避从头开始。
    ///
    /// ssh host 的重连在后台完成，命令立刻返回，状态随事实事件推进。
    ///
    /// # Errors
    ///
    /// - `StoreError::HostNotFound`：该名称不在 host 表中。
    /// - `StoreError::Database`：读取 host 表失败。
    pub async fn reconnect(
        self: &Arc<Self>,
        name: &HostName,
    ) -> Result<Option<HostSnapshot>, StoreError> {
        let Some(host) = self.store.host(name).await? else {
            return Ok(None);
        };
        tracing::info!(host = %host.name, "手动重连 host");
        self.connect(&host.name).await;
        Ok(Some(self.snapshot_of_now(host).await))
    }

    /// host 的当前状态。注册表里没有它时按不可达处理。
    pub async fn status(&self, name: &HostName) -> HostStatus {
        self.hosts
            .lock()
            .await
            .get(name)
            .map_or(HostStatus::Unreachable, |runtime| runtime.status)
    }

    /// 等 host 变成 `ok`，最多等到 `timeout`。启动参数里的 workspace 依赖它在登记前就绪。
    pub async fn await_ready(&self, name: &HostName, timeout: Duration) {
        let deadline = tokio::time::Instant::now() + timeout;
        while tokio::time::Instant::now() < deadline {
            if self.status(name).await == HostStatus::Ok {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// 请求因 host 不可用而失败时的说明：优先用状态说明，没有时给通用文案。
    pub async fn unavailable_detail(&self, name: &HostName) -> String {
        self.detail(name)
            .await
            .unwrap_or_else(|| format!("host {name} 当前不可用"))
    }

    /// host 的当前状态说明。
    pub async fn detail(&self, name: &HostName) -> Option<String> {
        self.hosts
            .lock()
            .await
            .get(name)
            .and_then(|runtime| runtime.detail.clone())
    }

    /// 让 host 校验一个绝对目录。`None` 表示这台 host 现在没有可用 link。
    pub async fn check_workspace(
        &self,
        name: &HostName,
        path: &str,
    ) -> Option<Result<String, WorkspaceCheckError>> {
        let link = self.link(name).await?;
        match link.check_workspace(path).await {
            Ok(result) => Some(result),
            Err(close) => {
                tracing::warn!(host = %name, detail = %close.detail(), "校验 workspace 时 link 已关闭");
                None
            }
        }
    }

    /// 这台 host 的 link。只有 `ok` 的 host 才有能用的 link。
    pub async fn link(&self, name: &HostName) -> Option<Arc<LinkHandle>> {
        let hosts = self.hosts.lock().await;
        let runtime = hosts.get(name)?;
        if !runtime.status.is_ok() {
            return None;
        }
        runtime.link.clone()
    }

    fn snapshot_of(host: HostMetadata, runtime: Option<&HostRuntime>) -> HostSnapshot {
        match runtime {
            Some(runtime) => HostSnapshot::of(
                host,
                runtime.status,
                runtime.detail.clone(),
                runtime.latency_ms,
            ),
            None => HostSnapshot::of(
                host,
                HostStatus::Unreachable,
                Some("尚未连接".to_owned()),
                None,
            ),
        }
    }

    async fn snapshot_of_now(&self, host: HostMetadata) -> HostSnapshot {
        let runtime = self.hosts.lock().await;
        let state = runtime.get(&host.name);
        Self::snapshot_of(host, state)
    }

    async fn ensure_local(&self) -> Result<(), StoreError> {
        let name = HostName::parse(LOCAL_HOST)
            .map_err(|error| StoreError::Invariant(format!("内置的 host 名不合法: {error}")))?;
        if self.store.host(&name).await?.is_none() {
            self.store
                .register_host(name, HostKind::Local, None)
                .await?;
            tracing::info!("登记本机 host");
        }
        Ok(())
    }

    async fn connect_all(self: &Arc<Self>) {
        match self.store.hosts().await {
            Ok(hosts) => {
                for host in hosts {
                    self.connect(&host.name).await;
                }
            }
            Err(error) => tracing::error!(error = %error, "读取 host 表失败，没有连接任何 host"),
        }
    }

    /// 按 host 行重建 link。停用的 host 只写状态，不拉子进程。
    ///
    /// 两种传输都由后台任务建立并保持：本机拉起子进程，ssh 先探测再安装。
    /// 注册或更改目标因此不会阻塞在网络上。
    async fn connect(self: &Arc<Self>, name: &HostName) {
        let host = match self.store.host(name).await {
            Ok(Some(host)) => host,
            Ok(None) => return,
            Err(error) => {
                tracing::error!(host = %name, error = %error, "读取 host 失败");
                return;
            }
        };
        if !host.enabled {
            self.clear_link(name).await;
            self.set_status(
                name,
                HostStatus::Unreachable,
                Some(DISABLED_DETAIL.to_owned()),
            )
            .await;
            return;
        }
        self.start_link(&host).await;
    }

    /// 为 host 换一个新的取消通道并返回接收端。
    ///
    /// 丢弃旧发送端让上一轮后台任务退出；新任务拿到的接收端只对下一轮动作生效。
    async fn fresh_cancel(&self, name: &HostName) -> watch::Receiver<()> {
        let mut hosts = self.hosts.lock().await;
        let runtime = hosts
            .entry(name.clone())
            .or_insert_with(HostRuntime::disconnected);
        let (sender, receiver) = watch::channel(());
        runtime.cancel = Some(sender);
        receiver
    }

    /// 丢掉 link 与后台任务并返回新的代号。停用、重建连接与重连都先走这里。
    ///
    /// 丢弃后台任务的取消通道让它醒来；旧 link 在这里被丢弃，
    /// 它的标准输入随之关闭。
    async fn clear_link(&self, name: &HostName) -> u64 {
        let mut hosts = self.hosts.lock().await;
        let runtime = hosts
            .entry(name.clone())
            .or_insert_with(HostRuntime::disconnected);
        runtime.generation += 1;
        runtime.link = None;
        runtime.latency_ms = None;
        runtime.cancel = None;
        runtime.generation
    }

    /// 换掉 link 并返回新的代号。旧 link 在这里被丢弃，它的标准输入随之关闭。
    async fn replace_link(&self, name: &HostName, link: Option<Arc<LinkHandle>>) -> u64 {
        let mut hosts = self.hosts.lock().await;
        let runtime = hosts
            .entry(name.clone())
            .or_insert_with(HostRuntime::disconnected);
        runtime.generation += 1;
        runtime.link = link;
        runtime.latency_ms = None;
        runtime.generation
    }

    /// link 关闭：按退出码判定 `unreachable` 或 `incompatible`。
    async fn link_closed(&self, name: &HostName, generation: u64, close: LinkClosed) {
        {
            let mut hosts = self.hosts.lock().await;
            let Some(runtime) = hosts.get_mut(name) else {
                return;
            };
            if runtime.generation != generation {
                tracing::debug!(host = %name, "关闭通知来自已被替换的 link");
                return;
            }
            runtime.link = None;
        }
        let (status, detail) = if close.protocol_mismatch() {
            (
                HostStatus::Incompatible,
                format!("帧协议版本不符（{}）", close.detail()),
            )
        } else {
            (HostStatus::Unreachable, close.detail())
        };
        tracing::info!(host = %name, ?status, "host 状态变化");
        self.set_status(name, status, Some(detail)).await;
    }

    /// 写状态并在变化时留下一条事实。延迟不进这个函数。
    async fn set_status(&self, name: &HostName, status: HostStatus, detail: Option<String>) {
        let changed = {
            let mut hosts = self.hosts.lock().await;
            let Some(runtime) = hosts.get_mut(name) else {
                return;
            };
            if runtime.status == status && runtime.detail == detail {
                false
            } else {
                runtime.status = status;
                runtime.detail.clone_from(&detail);
                true
            }
        };
        if !changed {
            return;
        }
        if let Err(error) = self.store.announce_host_status(name, status, detail).await {
            tracing::warn!(host = %name, error = %error, "写入 host 状态事实失败");
        }
    }

    fn spawn_heartbeat(self: &Arc<Self>) {
        let registry = Arc::clone(self);
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(registry.interval);
            ticker.tick().await;
            loop {
                ticker.tick().await;
                registry.heartbeat().await;
            }
        });
    }

    /// 一轮心跳：只对 `ok` 且空闲的 link 发，往返毫秒数写进延迟。
    async fn heartbeat(&self) {
        let targets: Vec<(HostName, Arc<LinkHandle>, u64)> = {
            let hosts = self.hosts.lock().await;
            hosts
                .iter()
                .filter(|(_, runtime)| runtime.status.is_ok())
                .filter_map(|(name, runtime)| {
                    runtime
                        .link
                        .as_ref()
                        .map(|link| (name.clone(), Arc::clone(link), runtime.generation))
                })
                .collect()
        };
        for (name, link, generation) in targets {
            if !link.is_idle().await {
                continue;
            }
            let seq = self.sequence.fetch_add(1, Ordering::SeqCst);
            let Some(latency) = link.ping(seq, self.interval).await else {
                continue;
            };
            let mut hosts = self.hosts.lock().await;
            if let Some(runtime) = hosts.get_mut(&name)
                && runtime.generation == generation
            {
                runtime.latency_ms = Some(latency);
            }
        }
    }
}
