use std::process::Command;

fn run_hub(listen: &str) -> std::process::Output {
    let directory = tempfile::tempdir().expect("创建临时目录");
    Command::new(env!("CARGO_BIN_EXE_hub"))
        .args(["--listen", listen, "--database"])
        .arg(directory.path().join("hub.sqlite"))
        .output()
        .expect("启动 hub 二进制")
}

#[test]
fn rejects_non_loopback_listen_address() {
    let output = run_hub("0.0.0.0:0");
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("stderr 是 UTF-8");
    assert!(stderr.contains("回环地址"), "stderr: {stderr}");
}

#[test]
fn rejects_unparsable_listen_address() {
    let output = run_hub("127.0.0.1:not-a-port");
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("stderr 是 UTF-8");
    assert!(stderr.contains("无法解析监听地址"), "stderr: {stderr}");
}

#[test]
fn default_listen_addresses_are_loopback() {
    for raw in [agent::DEFAULT_LISTEN, hub::DEFAULT_LISTEN] {
        let address = hub::parse_listen_address(raw).expect("默认监听地址合法");
        assert!(
            address.ip().is_loopback(),
            "默认监听地址不是回环地址: {raw}"
        );
    }
}
