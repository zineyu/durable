use std::process::Command;

fn run_agent(listen: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_agent"))
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
