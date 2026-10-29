//! 断链说明：executor 的退出码与最后几行标准错误。
//! link 的读写与请求配对在父模块；host 三态由 `crate::hosts` 装配。

use std::fmt;

/// executor 表示帧协议或构建版本不符的退出码（见 `crates/executor`）。
pub const INCOMPATIBLE_EXIT_CODE: i32 = 3;

/// link 已断开：executor 的退出码与最后几行标准错误。
///
/// 只由本模块产生。工具结局里的 [`durable::ToolErrorCode::LinkClosed`] 是它在工具层的对应码，
/// 但工具实现从不产生该码；引擎据本结构把在途调用记为「结果未知」。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkClosed {
    /// executor 的退出码。被信号终止或退出码未及时到达时为 `None`。
    pub exit_code: Option<i32>,
    /// executor 标准错误的最后若干行，用于说明断开原因。
    pub stderr_tail: String,
}

impl LinkClosed {
    /// 退出码 3 表示帧协议或构建版本不符，host 据此成为 `incompatible`。
    #[must_use]
    pub fn protocol_mismatch(&self) -> bool {
        self.exit_code == Some(INCOMPATIBLE_EXIT_CODE)
    }

    /// 没有退出码与标准错误可依据的断开：等待者被丢弃但状态没来得及写上。
    pub(super) fn lost() -> Self {
        Self {
            exit_code: None,
            stderr_tail: String::new(),
        }
    }

    /// 写进 host 状态的说明：退出码与标准错误尾部。
    #[must_use]
    pub fn detail(&self) -> String {
        let tail = self.stderr_tail.trim();
        match (self.exit_code, tail.is_empty()) {
            (Some(code), false) => format!("executor 退出码 {code}：{tail}"),
            (Some(code), true) => format!("executor 退出码 {code}"),
            (None, false) => tail.to_owned(),
            (None, true) => "link 已断开".to_owned(),
        }
    }

    /// 请求没有可用 link 时的断开原因：host 停用、不可达或版本不符。
    #[must_use]
    pub fn unavailable(detail: String) -> Self {
        Self {
            exit_code: None,
            stderr_tail: detail,
        }
    }
}

impl fmt::Display for LinkClosed {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("link 已断开")?;
        if let Some(code) = self.exit_code {
            write!(formatter, "（executor 退出码 {code}）")?;
        }
        let tail = self.stderr_tail.trim();
        if !tail.is_empty() {
            write!(formatter, ": {tail}")?;
        }
        Ok(())
    }
}

impl std::error::Error for LinkClosed {}
