//! link 帧：agent 与 executor 在标准输入输出上交换的长度前缀 JSON 帧。
//! 帧的驱动在 `crates/agent/src/link`，工具执行在 `crates/executor`。

use std::io;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::ToolOutcome;

/// 帧协议版本。两侧通过启动参数比对，不符即 `incompatible`。
pub const LINK_PROTOCOL_VERSION: u32 = 1;

/// 单帧正文上限。超限说明链路已损坏，而不是某次工具输出过大。
pub const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;

const LENGTH_BYTES: usize = 4;

/// link 上的一种帧。
///
/// 一次工具调用是一帧请求与一帧结果，按 `id` 配对；取消按同一 `id` 匹配，
/// 被取消的请求仍回一帧结果；心跳用于保活与量延迟。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "frame",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum Frame {
    Request {
        id: String,
        workspace: String,
        tool: String,
        arguments: String,
    },
    Result {
        id: String,
        outcome: ToolOutcome,
    },
    Cancel {
        id: String,
    },
    Ping {
        seq: u64,
    },
    Pong {
        seq: u64,
    },
}

/// 写一帧：4 字节大端长度，然后是 UTF-8 JSON 正文。
///
/// # Errors
///
/// - 帧无法序列化或超过 [`MAX_FRAME_BYTES`] 时返回 [`io::ErrorKind::InvalidData`]。
/// - 写入失败时返回该错误。
pub async fn write_frame<W: AsyncWrite + Unpin>(writer: &mut W, frame: &Frame) -> io::Result<()> {
    let payload =
        serde_json::to_vec(frame).map_err(|error| invalid(format!("帧无法编码: {error}")))?;
    if payload.len() > MAX_FRAME_BYTES {
        return Err(invalid(format!(
            "帧 {} 字节，超过 {MAX_FRAME_BYTES} 字节上限",
            payload.len()
        )));
    }
    let length = u32::try_from(payload.len())
        .map_err(|_| invalid(format!("帧 {} 字节，无法编码长度", payload.len())))?;
    writer.write_all(&length.to_be_bytes()).await?;
    writer.write_all(&payload).await?;
    writer.flush().await?;
    Ok(())
}

/// 读一帧。对端正常关闭时返回 `Ok(None)`。
///
/// # Errors
///
/// - 帧长度或正文被截断、长度超过 [`MAX_FRAME_BYTES`]、正文不是合法 JSON 时返回
///   [`io::ErrorKind::InvalidData`]。
/// - 读取失败时返回该错误。
pub async fn read_frame<R: AsyncRead + Unpin>(reader: &mut R) -> io::Result<Option<Frame>> {
    let Some(length) = read_length(reader).await? else {
        return Ok(None);
    };
    let size =
        usize::try_from(u32::from_be_bytes(length)).map_err(|_| invalid("帧长度无法转成 usize"))?;
    if size > MAX_FRAME_BYTES {
        return Err(invalid(format!(
            "帧 {size} 字节，超过 {MAX_FRAME_BYTES} 字节上限"
        )));
    }
    let mut payload = vec![0u8; size];
    reader.read_exact(&mut payload).await.map_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            invalid(format!("帧正文被截断：期望 {size} 字节"))
        } else {
            error
        }
    })?;
    let frame = serde_json::from_slice(&payload)
        .map_err(|error| invalid(format!("帧不是合法 JSON: {error}")))?;
    Ok(Some(frame))
}

/// 读 4 字节长度。首字节之前遇到 EOF 是正常关闭，读到一半遇到 EOF 即截断。
async fn read_length<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> io::Result<Option<[u8; LENGTH_BYTES]>> {
    let mut length = [0u8; LENGTH_BYTES];
    let mut filled = 0;
    while filled < LENGTH_BYTES {
        let room = length
            .get_mut(filled..)
            .ok_or_else(|| invalid(format!("帧长度只读到 {filled} 字节")))?;
        let read = reader.read(room).await?;
        if read == 0 {
            if filled == 0 {
                return Ok(None);
            }
            return Err(invalid(format!("帧长度只读到 {filled} 字节")));
        }
        filled += read;
    }
    Ok(Some(length))
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ToolErrorCode;

    async fn round_trip(frame: Frame) {
        let mut buffer = Vec::new();
        write_frame(&mut buffer, &frame).await.expect("写入帧");
        let mut reader: &[u8] = &buffer;
        assert_eq!(
            read_frame(&mut reader).await.expect("读取帧"),
            Some(frame),
            "帧往返一致"
        );
        assert_eq!(
            read_frame(&mut reader).await.expect("读到流末尾"),
            None,
            "末尾返回 None"
        );
    }

    #[tokio::test]
    async fn every_frame_variant_round_trips() {
        round_trip(Frame::Request {
            id: "req-1".into(),
            workspace: "/tmp/workspace".into(),
            tool: "bash".into(),
            arguments: r#"{"command":"echo 你好"}"#.into(),
        })
        .await;
        round_trip(Frame::Result {
            id: "req-1".into(),
            outcome: ToolOutcome::Succeeded {
                content: "输出".into(),
            },
        })
        .await;
        round_trip(Frame::Result {
            id: "req-2".into(),
            outcome: ToolOutcome::Failed {
                code: ToolErrorCode::BashTimeout,
                message: "命令超过 120000 ms 未结束".into(),
            },
        })
        .await;
        round_trip(Frame::Cancel { id: "req-1".into() }).await;
        round_trip(Frame::Ping { seq: 7 }).await;
        round_trip(Frame::Pong { seq: 7 }).await;
    }

    #[tokio::test]
    async fn clean_eof_is_not_an_error() {
        let mut empty: &[u8] = &[];
        assert_eq!(read_frame(&mut empty).await.expect("空流正常结束"), None);
    }

    #[tokio::test]
    async fn oversized_length_is_rejected() {
        let oversized = u32::try_from(MAX_FRAME_BYTES + 1).expect("上限可编码");
        let mut reader: &[u8] = &oversized.to_be_bytes();
        let error = read_frame(&mut reader).await.expect_err("超限被拒绝");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[tokio::test]
    async fn truncated_length_is_rejected() {
        let mut reader: &[u8] = &[0, 0];
        let error = read_frame(&mut reader).await.expect_err("截断被拒绝");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[tokio::test]
    async fn invalid_json_is_rejected() {
        let payload = b"{".to_vec();
        let length = u32::try_from(payload.len()).expect("长度可编码");
        let mut buffer = length.to_be_bytes().to_vec();
        buffer.extend_from_slice(&payload);
        let mut reader: &[u8] = &buffer;
        let error = read_frame(&mut reader).await.expect_err("非法 JSON 被拒绝");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }
}
