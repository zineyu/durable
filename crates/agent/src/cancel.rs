//! 取消信号：请求与工具共享的一次性通知。
//! 杀进程组在 `tools/bash`。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::Notify;

/// 可克隆的取消信号。取消后所有等待者被唤醒。
#[derive(Clone, Default)]
pub struct Cancellation {
    inner: Arc<Inner>,
}

#[derive(Default)]
struct Inner {
    cancelled: AtomicBool,
    notify: Notify,
}

impl Cancellation {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.inner.cancelled.store(true, Ordering::SeqCst);
        self.inner.notify.notify_waiters();
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::SeqCst)
    }

    /// 等待取消。
    pub async fn cancelled(&self) {
        loop {
            let notified = self.inner.notify.notified();
            if self.is_cancelled() {
                return;
            }
            notified.await;
        }
    }
}
