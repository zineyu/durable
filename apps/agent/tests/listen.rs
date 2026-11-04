#![expect(
    clippy::expect_used,
    reason = "监听地址测试的启动夹具用 expect 直接报告失败。"
)]

mod support;

fn run_agent(listen: &str) -> std::process::Output {
    let directory = tempfile::tempdir().expect("创建临时目录");
    support::command(&directory.path().join("agent.sqlite"))
        .args(["--listen", listen])
        .output()
        .expect("启动 agent 二进制")
}

#[test]
fn rejects_non_loopback_listen_address() {
    let output = run_agent("0.0.0.0:0");
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("stderr 是 UTF-8");
    assert!(stderr.contains("回环地址"), "stderr: {stderr}");
}

#[test]
fn rejects_unparsable_listen_address() {
    let output = run_agent("127.0.0.1:not-a-port");
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("stderr 是 UTF-8");
    assert!(stderr.contains("无法解析监听地址"), "stderr: {stderr}");
}
