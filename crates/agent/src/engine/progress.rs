//! 流式正文的节流：决定何时把累积文本写入 run 的 progress。
//! 写入调用在父模块的流循环，落库在 `durable` 的 run 写命令。

use std::time::{Duration, Instant};

/// 流式正文的节流器。按时间间隔与内容变化决定是否提交。
///
/// 时间由调用方传入，因此判定是确定性的：测试不需要等待真实时间。
pub(super) struct ProgressThrottle {
    interval: Duration,
    /// 上次放行的时刻；`None` 表示本次步骤还没提交过。
    last: Option<Instant>,
    /// 上次放行的正文。内容未变时不再提交，重复的正文对恢复没有意义。
    committed: String,
}

impl ProgressThrottle {
    pub(super) fn new(interval: Duration) -> Self {
        Self {
            interval,
            last: None,
            committed: String::new(),
        }
    }

    /// 本次应当提交的正文；未到间隔、正文为空或正文未变时返回 `None`。
    ///
    /// 返回 `Some` 即视为已提交：调用方随后写入进度，不论写入是否成功。
    pub(super) fn take(&mut self, text: &str, now: Instant) -> Option<String> {
        if text.is_empty() || text == self.committed {
            return None;
        }
        if let Some(last) = self.last
            && now.saturating_duration_since(last) < self.interval
        {
            return None;
        }
        self.last = Some(now);
        text.clone_into(&mut self.committed);
        Some(self.committed.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one_second() -> Duration {
        Duration::from_secs(1)
    }

    #[test]
    fn first_text_is_due_immediately() {
        let mut throttle = ProgressThrottle::new(one_second());
        let now = Instant::now();
        assert_eq!(throttle.take("第一段", now).as_deref(), Some("第一段"));
    }

    #[test]
    fn text_within_the_interval_is_held_back() {
        let mut throttle = ProgressThrottle::new(one_second());
        let now = Instant::now();
        throttle.take("第一段", now);
        assert_eq!(
            throttle.take("第一段第二句", now + Duration::from_millis(500)),
            None
        );
        assert_eq!(
            throttle.take("第一段第二句", now + one_second()).as_deref(),
            Some("第一段第二句"),
            "到达间隔后放行最新的累积正文"
        );
    }

    #[test]
    fn unchanged_text_is_never_due_again() {
        let mut throttle = ProgressThrottle::new(one_second());
        let now = Instant::now();
        throttle.take("第一段", now);
        assert_eq!(throttle.take("第一段", now + Duration::from_secs(10)), None);
    }

    #[test]
    fn empty_text_is_never_due() {
        let mut throttle = ProgressThrottle::new(one_second());
        assert_eq!(throttle.take("", Instant::now()), None);
    }
}
