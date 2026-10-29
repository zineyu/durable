//! ssh host 的连接：后台任务里的探测、安装、拉起 `ssh` 子进程与退避重试。
//! 本机传输、状态事实与心跳在父模块；远端脚本与载荷在 `crate::link::ssh`。
//!
//! 一个 ssh host 只有一条 link、一个后台任务。用户动作（重连、改目标、停用、删除）
//! 通过丢弃 [`watch::Sender`] 让任务在 [`cancelled`] 上醒来，退避因此不会拖慢手动重连。

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use durable::{HostMetadata, HostName, HostStatus};
use tokio::sync::watch;

use super::HostRegistry;
use crate::link::ssh::{self, SetupFailure};
use crate::link::{BUILD_ID, LinkHandle, Transport};

/// ssh host 注册后、第一条 link 建立前的状态说明。
const CONNECTING_DETAIL: &str = "正在连接";

/// ssh 断链后重连的退避起点。每次失败翻倍，封顶 [`RETRY_CAP`]。
const RETRY_BASE: Duration = Duration::from_secs(1);

/// ssh 重连退避的上限。
const RETRY_CAP: Duration = Duration::from_secs(30);

impl HostRegistry {
    /// ssh 传输：先回到「正在连接」，把安装、连接与退避重试交给后台任务。
    ///
    /// 丢掉的旧任务在 [`cancelled`] 上醒来即退出，手动重连因此立即生效（退避从头算）。
    pub(super) async fn start_ssh(self: &Arc<Self>, host: &HostMetadata) {
        self.clear_link(&host.name).await;
        let cancel = self.fresh_cancel(&host.name).await;
        self.set_status(
            &host.name,
            HostStatus::Unreachable,
            Some(CONNECTING_DETAIL.to_owned()),
        )
        .await;
        tokio::spawn(Arc::clone(self).ssh_supervisor(host.name.clone(), cancel));
    }

    /// ssh host 的后台连接任务：连上就等它断开，断开或连不上就退避后重试。
    ///
    /// 退避为 1 秒起、逐次翻倍、封顶 30 秒，并加一半以内的抖动。
    /// 用户动作（重连、改目标、停用、删除）会让 [`cancelled`] 醒来，任务随即退出。
    async fn ssh_supervisor(self: Arc<Self>, name: HostName, mut cancel: watch::Receiver<()>) {
        let mut backoff = RETRY_BASE;
        loop {
            let Some(host) = self.host_row(&name).await else {
                return;
            };
            if !host.enabled {
                return;
            }
            match self.open_ssh(&host).await {
                Ok(link) => {
                    let generation = self.replace_link(&name, Some(Arc::clone(&link))).await;
                    self.set_status(&name, HostStatus::Ok, None).await;
                    let close = tokio::select! {
                        close = link.wait_closed() => close,
                        () = cancelled(&mut cancel) => return,
                    };
                    drop(link);
                    self.link_closed(&name, generation, close).await;
                }
                Err((status, detail)) => {
                    self.replace_link(&name, None).await;
                    self.set_status(&name, status, Some(detail)).await;
                }
            }
            let delay = jitter(backoff);
            tracing::info!(
                host = %name,
                delay_ms = delay.as_millis(),
                "ssh host 将在退避后重连"
            );
            tokio::select! {
                () = tokio::time::sleep(delay) => {}
                () = cancelled(&mut cancel) => return,
            }
            backoff = (backoff * 2).min(RETRY_CAP);
        }
    }

    /// 建立一条 ssh link：必要时探测平台并安装 executor，然后拉起 `ssh` 子进程。
    async fn open_ssh(&self, host: &HostMetadata) -> Result<Arc<LinkHandle>, (HostStatus, String)> {
        let Some(target) = host.target.as_deref() else {
            return Err((HostStatus::Incompatible, "ssh host 缺少目标".to_owned()));
        };
        let installed = match self.store.host_installed_build(&host.name).await {
            Ok(build) => build,
            Err(error) => {
                tracing::warn!(host = %host.name, error = %error, "读取 installed_build 失败，按未安装处理");
                None
            }
        };
        let installed_now = ssh::prepare(target, installed.as_deref(), &self.executor)
            .await
            .map_err(status_of)?;
        if installed_now {
            tracing::info!(host = %host.name, build = BUILD_ID, "已安装 executor");
            if let Err(error) = self
                .store
                .set_host_installed_build(&host.name, BUILD_ID)
                .await
            {
                tracing::warn!(host = %host.name, error = %error, "记录 installed_build 失败");
            }
        }
        LinkHandle::spawn(
            &Transport::Ssh {
                target: target.to_owned(),
            },
            self.limits,
            &host.name,
        )
        .map(Arc::new)
        .map_err(|error| (HostStatus::Unreachable, format!("启动 ssh 失败: {error}")))
    }

    /// 读 host 行。读不到（已删除或数据库错误）时不重试。
    async fn host_row(&self, name: &HostName) -> Option<HostMetadata> {
        match self.store.host(name).await {
            Ok(host) => host,
            Err(error) => {
                tracing::error!(host = %name, error = %error, "读取 host 失败");
                None
            }
        }
    }
}

/// ssh 准备失败 → host 状态与说明。
fn status_of(failure: SetupFailure) -> (HostStatus, String) {
    match failure {
        SetupFailure::Incompatible(detail) => (HostStatus::Incompatible, detail),
        SetupFailure::Unreachable(detail) => (HostStatus::Unreachable, detail),
    }
}

/// 等下一个连接代号或发送端关闭：两者都表示这轮后台任务该退出。
pub(super) async fn cancelled(cancel: &mut watch::Receiver<()>) {
    drop(cancel.changed().await);
}

/// 在退避时长上加一半以内的抖动，避免多台 ssh host 同时重连。
fn jitter(delay: Duration) -> Duration {
    let spread = u64::try_from(delay.as_millis()).unwrap_or(u64::MAX) / 2;
    let offset = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| u64::from(elapsed.subsec_nanos()));
    delay + Duration::from_millis(offset % spread.max(1))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{RETRY_BASE, RETRY_CAP, jitter};

    #[test]
    fn jitter_stays_within_the_backoff_bound() {
        for base in [RETRY_BASE, Duration::from_secs(7), RETRY_CAP] {
            for _ in 0..20 {
                let delay = jitter(base);
                assert!(delay >= base, "{delay:?} 不短于退避");
                assert!(delay < base * 2, "{delay:?} 不超过退避的一倍");
            }
        }
    }
}
