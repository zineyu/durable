//! executor 二进制的进程级契约：握手版本、心跳应答与标准输入 EOF 收尾。
//! 工具行为经 agent 的 HTTP 断言，见 `crates/agent/tests/tools.rs`。

#![expect(
    clippy::expect_used,
    reason = "帧级测试用 expect 直接报告夹具与断言失败。"
)]

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use durable::link::{Frame, LINK_PROTOCOL_VERSION, read_frame, write_frame};
use tokio::process::{Child, Command};

fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_executor"))
}

fn command(protocol: u32) -> Command {
    let mut command = Command::new(binary());
    command
        .arg("--link-protocol")
        .arg(protocol.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    command
}

fn spawn(protocol: u32) -> Child {
    command(protocol).spawn().expect("启动 executor")
}

async fn send(child: &mut Child, frame: &Frame) {
    let stdin = child.stdin.as_mut().expect("标准输入已打开");
    write_frame(stdin, frame).await.expect("写入帧");
}

async fn receive(child: &mut Child) -> Option<Frame> {
    let stdout = child.stdout.as_mut().expect("标准输出已打开");
    read_frame(stdout).await.expect("读取帧")
}

/// 规格 user story R-53：帧协议版本不符即退出，调用方据此判定 `incompatible`。
#[tokio::test]
async fn incompatible_protocol_exits_with_code_three() {
    let child = spawn(LINK_PROTOCOL_VERSION + 1);
    let output = child.wait_with_output().await.expect("等待 executor 退出");
    assert_eq!(
        output.status.code(),
        Some(3),
        "协议版本不符以 3 退出，实际 {:?}",
        output.status
    );
}

/// 规格 user story R-42：标准输入 EOF 后收尾并以 0 退出。
#[tokio::test]
async fn answers_ping_and_exits_on_eof() {
    let mut child = spawn(LINK_PROTOCOL_VERSION);
    send(&mut child, &Frame::Ping { seq: 42 }).await;
    assert_eq!(
        receive(&mut child).await,
        Some(Frame::Pong { seq: 42 }),
        "心跳得到同序号的应答"
    );

    drop(child.stdin.take());
    let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .expect("EOF 后按时退出")
        .expect("等待 executor 退出");
    assert!(status.success(), "干净 EOF 以 0 退出，实际 {status:?}");
}
