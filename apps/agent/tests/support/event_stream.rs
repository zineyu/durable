//! 已订阅的 SSE 事件流：按 `data:` 行产出协议事件。
//! 订阅入口在 `mod` 的 `Harness::subscribe`。

use super::*;

/// 已订阅的事件流，按 SSE `data:` 行产出协议事件。
pub(crate) struct EventStream {
    pub(super) response: Response,
    pub(super) buffer: Vec<u8>,
}

impl EventStream {
    pub(crate) async fn next(&mut self) -> Option<agent::StreamEvent> {
        loop {
            if let Some(position) = self.buffer.iter().position(|byte| *byte == b'\n') {
                let line: Vec<u8> = self.buffer.drain(..=position).collect();
                let line = String::from_utf8(line).expect("事件行是 UTF-8");
                let Some(data) = line.trim_end().strip_prefix("data:") else {
                    continue;
                };
                let data = data.trim();
                if data.is_empty() {
                    continue;
                }
                return Some(serde_json::from_str(data).expect("解析事件"));
            }
            let chunk = self.response.chunk().await.expect("读取事件流")?;
            self.buffer.extend_from_slice(&chunk);
        }
    }
}

pub(crate) async fn error_code(response: Response) -> agent::ErrorCode {
    error_envelope(response).await.code
}

pub(crate) async fn error_envelope(response: Response) -> agent::ErrorResponse {
    response.json().await.expect("解析错误响应")
}
