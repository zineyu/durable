//! host 的连接：两种传输的后台任务、退避重试，以及 ssh 侧的探测与安装。
//! 状态迁移、link 与代号在 `super::lifecycle`；远端脚本与载荷在 `crate::link::ssh`。
//!
//! 一个 host 只有一条 link、一个后台任务。用户动作（重连、更改目标、停用、删除）
//! 通过丢弃 [`watch::Sender`] 让任务在 [`cancelled`] 上醒来，退避因此不会拖慢手动重连。
//!
//! 本模块只报告一次尝试的结果：建成 link、尝试失败、link 断开。状态由 lifecycle 决定。

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use durable::{HostKind, HostMetadata, HostName};
use tokio::sync::watch;

use super::HostRegistry;
use super::lifecycle::{Attempt, HostEvent, HostFailure};
use crate::link::ssh::{self, SetupFailure};
use crate::link::{BUILD_ID, LinkHandle, Transport};

/// 断链后重连的退避起点。每次失败翻倍，封顶 [`RETRY_CAP`]。
const RETRY_BASE: Duration = Duration::from_secs(1);

/// 重连退避的上限。
const RETRY_CAP: Duration = Duration::from_secs(30);

impl HostRegistry {
    /// 建立 link：让 lifecycle 签一个新代号并进入「正在连接」，把连接与退避交给后台任务。
    ///
    /// 丢掉的旧任务在 [`cancelled`] 上醒来立即退出，手动重连因此立即生效（退避从头算）。
    pub(super) async fn start_link(self: &Arc<Self>, host: &HostMetadata) {
        let Some(applied) = self.transition(&host.name, HostEvent::Connecting).await else {
            return;
        };
        let Some((attempt, cancel)) = applied.attempt else {
            return;
        };
        tokio::spawn(Arc::clone(self).supervisor(host.name.clone(), attempt, cancel));
    }

    /// host 的后台连接任务：连上就等待它断开，断开或连不上就退避后重试。
    ///
    /// 退避为 1 秒起、逐次翻倍、封顶 30 秒，并加一半以内的抖动。
    /// 用户动作（重连、更改目标、停用、删除）会让 [`cancelled`] 醒来，任务随即退出；
    /// 代号被 lifecycle 作废时，任务报告的事件不被采纳，任务同样退出。
    async fn supervisor(
        self: Arc<Self>,
        name: HostName,
        attempt: Attempt,
        mut cancel: watch::Receiver<()>,
    ) {
        let mut backoff = RETRY_BASE;
        loop {
            let Some(host) = self.host_row(&name).await else {
                return;
            };
            if !host.enabled {
                return;
            }
            let opened = match host.kind {
                HostKind::Local => self.open_local(&host),
                HostKind::Ssh => self.open_ssh(&host).await,
            };
            match opened {
                Ok(link) => {
                    let opened = self
                        .transition(&name, HostEvent::Opened(attempt, Arc::clone(&link)))
                        .await;
                    match opened {
                        Some(applied) if applied.accepted => {}
                        Some(_) | None => return,
                    }
                    let close = tokio::select! {
                        close = link.wait_closed() => close,
                        () = cancelled(&mut cancel) => return,
                    };
                    drop(link);
                    self.transition(&name, HostEvent::Close(attempt, close))
                        .await;
                }
                Err(failure) => {
                    let failed = self
                        .transition(&name, HostEvent::AttemptFailed(attempt, failure))
                        .await;
                    match failed {
                        Some(applied) if applied.accepted => {}
                        Some(_) | None => return,
                    }
                }
            }
            let delay = jitter(backoff);
            tracing::info!(
                host = %name,
                delay_ms = delay.as_millis(),
                "host 将在退避后重连"
            );
            tokio::select! {
                () = tokio::time::sleep(delay) => {}
                () = cancelled(&mut cancel) => return,
            }
            backoff = (backoff * 2).min(RETRY_CAP);
        }
    }

    /// 本机 link：直接拉起 executor 子进程。
    fn open_local(&self, host: &HostMetadata) -> Result<Arc<LinkHandle>, HostFailure> {
        LinkHandle::spawn(
            &Transport::Local {
                executor: self.executor.clone(),
            },
            self.limits,
            &host.name,
        )
        .map(Arc::new)
        .map_err(|error| HostFailure::Unreachable(format!("启动 executor 失败: {error}")))
    }

    /// 建立一条 ssh link：必要时探测平台并安装 executor，然后拉起 `ssh` 子进程。
    async fn open_ssh(&self, host: &HostMetadata) -> Result<Arc<LinkHandle>, HostFailure> {
        let Some(target) = host.target.as_deref() else {
            return Err(HostFailure::Incompatible("ssh host 缺少目标".to_owned()));
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
            .map_err(failure_of)?;
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
        .map_err(|error| HostFailure::Unreachable(format!("启动 ssh 失败: {error}")))
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

/// ssh 准备失败 → 这次尝试失败的原因。
fn failure_of(failure: SetupFailure) -> HostFailure {
    match failure {
        SetupFailure::Incompatible(detail) => HostFailure::Incompatible(detail),
        SetupFailure::Unreachable(detail) => HostFailure::Unreachable(detail),
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
