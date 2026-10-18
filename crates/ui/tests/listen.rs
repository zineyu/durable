use std::process::Command;

fn run_ui(listen: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ui"))
        .args(["--listen", listen])
        .output()
        .expect("启动 ui 二进制")
}

#[test]
fn rejects_non_loopback_listen_address() {
    let output = run_ui("0.0.0.0:0");
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("stderr 是 UTF-8");
    assert!(stderr.contains("回环地址"), "stderr: {stderr}");
}

#[test]
fn rejects_unparsable_listen_address() {
    let output = run_ui("127.0.0.1:not-a-port");
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("stderr 是 UTF-8");
    assert!(stderr.contains("无法解析监听地址"), "stderr: {stderr}");
}

#[test]
fn default_listen_addresses_are_loopback() {
    for raw in [agent::DEFAULT_LISTEN, ui::DEFAULT_LISTEN] {
        let address = ui::parse_listen_address(raw).expect("默认监听地址合法");
        assert!(
            address.ip().is_loopback(),
            "默认监听地址不是回环地址: {raw}"
        );
    }
}
