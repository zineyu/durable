//! 规格 user story R-1、R-2、R-12、R-14、R-15、R-16、R-28、R-35：
//! ssh host 的注册、自动安装与工具执行经真实 `agent` 二进制验证。
//!
//! 传输的替身只有 `ssh` 本身：`PATH` 首位是一个独立的假 ssh，它把 argv 记进日志，
//! 再把远端命令交给本机的 `sh`。`agent`、`executor` 与帧协议都是真的，
//! 因此安装、启动与工具调用走的是完整的 link 路径。

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "进程级测试用 expect 与 panic 直接报告夹具与断言失败。"
)]

use std::time::Duration;

use axum::http::StatusCode;
use reqwest::Response;

mod support;

use support::process::{Process, create_session_on, nodes_of, submit, wait_idle};
use support::{FakeSsh, Script, ScriptedProvider, installed_build};

const HOST: &str = "remote";
const TARGET: &str = "example-host";

/// 启动 agent 进程，把假 ssh 放到 `PATH` 首位。
async fn start(
    ssh: &FakeSsh,
    scripts: Vec<Script>,
) -> (Process, tempfile::TempDir, ScriptedProvider) {
    let provider = ScriptedProvider::start(scripts).await;
    let directory = tempfile::tempdir().expect("创建临时目录");
    let database = directory.path().join("agent.sqlite");
    let workspace = directory.path().join("workspace");
    std::fs::create_dir(&workspace).expect("创建 workspace 目录");
    let environment = [("PATH", ssh.path_environment())];
    let process =
        Process::start_with_environment(&database, &workspace, &provider, &environment).await;
    (process, directory, provider)
}

async fn register_host(process: &Process, name: &str, target: &str) -> Response {
    reqwest::Client::new()
        .post(process.url("/api/hosts"))
        .json(&serde_json::json!({ "name": name, "kind": "ssh", "target": target }))
        .send()
        .await
        .expect("注册 host")
}

/// 轮询 bootstrap 直到 host 快照满足条件。
async fn wait_host(
    process: &Process,
    name: &str,
    ready: impl Fn(&agent::HostSnapshot) -> bool,
) -> agent::HostSnapshot {
    let mut last = None;
    for _ in 0..500 {
        let bootstrap: agent::BootstrapResponse = reqwest::get(process.url("/api/bootstrap"))
            .await
            .expect("读取 bootstrap")
            .json()
            .await
            .expect("解析 bootstrap");
        if let Some(host) = bootstrap
            .hosts
            .iter()
            .find(|host| host.name.as_str() == name && ready(host))
        {
            return host.clone();
        }
        last = bootstrap
            .hosts
            .into_iter()
            .find(|host| host.name.as_str() == name);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("host {name} 未在预期时间内满足条件: {last:?}");
}

async fn reconnect(process: &Process, name: &str) {
    let response = reqwest::Client::new()
        .post(process.url(&format!("/api/hosts/{name}/reconnect")))
        .send()
        .await
        .expect("重连 host");
    assert_eq!(response.status(), StatusCode::OK, "重连返回 200");
}

/// 规格 user story R-1、R-2、R-14：ssh host 一次连上，argv 是 `ssh <target> sh -c '<脚本>'`，
/// 安装只发生一次，`installed_build` 记下这次构建。
#[tokio::test]
async fn ssh_host_installs_then_links_with_the_expected_argv() {
    let ssh = FakeSsh::working();
    let (process, _directory, _provider) = start(&ssh, Vec::new()).await;
    assert_eq!(
        register_host(&process, HOST, TARGET).await.status(),
        StatusCode::CREATED
    );

    let host = wait_host(&process, HOST, |host| host.status.is_ok()).await;
    assert!(host.enabled, "新 host 默认启用");
    assert_eq!(host.kind, agent::HostKind::Ssh);
    assert_eq!(host.target.as_deref(), Some(TARGET));
    assert!(host.detail.is_none(), "ok 时没有说明");

    let invocations = ssh.invocations();
    assert!(!invocations.is_empty(), "agent 调用过假 ssh");
    for invocation in &invocations {
        assert_eq!(
            invocation.arguments.first().map(String::as_str),
            Some(TARGET),
            "目标始终是第一个参数: {:?}",
            invocation.arguments
        );
    }
    let probe = invocations
        .iter()
        .position(|invocation| {
            invocation
                .arguments
                .iter()
                .any(|argument| argument.contains("uname -sm"))
        })
        .expect("先探测远端平台");
    assert_eq!(probe, 0, "平台探测发生在安装与启动之前");

    let launch = ssh
        .wait_for_invocation(|invocation| {
            invocation
                .arguments
                .iter()
                .any(|argument| argument.contains("--link-protocol"))
        })
        .await;
    assert_eq!(&launch.arguments[1..3], ["sh", "-c"], "远端命令经 sh -c");
    let script = &launch.arguments[3];
    assert!(script.contains("exec \"$d/executor-"), "{script}");
    assert!(
        script.contains(agent::BUILD_ID),
        "远端文件名带构建 id: {script}"
    );

    assert_eq!(ssh.installs(), 1, "首次连接安装一次");
    assert_eq!(
        installed_build(process.database(), HOST).await.as_deref(),
        Some(agent::BUILD_ID),
        "host 行记下已安装的构建"
    );
}

/// 规格 user story R-7：换掉 SSH 目标可能已是另一台机器，已安装的构建因此不再可信。
#[tokio::test]
async fn changing_the_target_reinstalls_the_executor() {
    let ssh = FakeSsh::working();
    let (process, _directory, _provider) = start(&ssh, Vec::new()).await;
    assert_eq!(
        register_host(&process, HOST, TARGET).await.status(),
        StatusCode::CREATED
    );
    wait_host(&process, HOST, |host| host.status.is_ok()).await;
    let installs = ssh.installs();
    assert!(installs >= 1, "首次连接安装");

    let response = reqwest::Client::new()
        .patch(process.url(&format!("/api/hosts/{HOST}")))
        .json(&serde_json::json!({ "target": "other-host" }))
        .send()
        .await
        .expect("更改目标");
    assert_eq!(response.status(), StatusCode::OK);
    let host = wait_host(&process, HOST, |host| {
        host.target.as_deref() == Some("other-host") && host.status.is_ok()
    })
    .await;
    assert!(host.detail.is_none());
    assert_eq!(ssh.installs(), installs + 1, "新目标重新安装");
}

/// 规格 user story R-16：ssh 退出码 3 表示两侧版本不符，host 成为 `incompatible`，
/// 说明里带着远端写的原文。
#[tokio::test]
async fn protocol_mismatch_marks_the_host_incompatible() {
    let ssh = FakeSsh::failing(3, "帧协议版本不符：executor 期望 1，收到 2");
    let (process, _directory, _provider) = start(&ssh, Vec::new()).await;
    assert_eq!(
        register_host(&process, HOST, TARGET).await.status(),
        StatusCode::CREATED
    );

    let host = wait_host(&process, HOST, |host| {
        host.status == agent::HostStatus::Incompatible
    })
    .await;
    let detail = host.detail.expect("不兼容状态带说明");
    assert!(detail.contains("帧协议版本不符"), "{detail}");
    assert!(detail.contains("期望 1"), "说明保留远端原文: {detail}");
}

/// 规格 user story R-16：远端平台没有对应载荷时 host 是 `incompatible`，说明里写出两侧平台。
#[tokio::test]
async fn a_remote_platform_without_a_payload_is_incompatible() {
    let (uname, tag) = if agent::BUILD_PLATFORM.starts_with("linux") {
        ("Darwin arm64", "darwin-aarch64")
    } else {
        ("Linux x86_64", "linux-x86_64")
    };
    let ssh = FakeSsh::reporting(uname);
    let (process, _directory, _provider) = start(&ssh, Vec::new()).await;
    assert_eq!(
        register_host(&process, HOST, TARGET).await.status(),
        StatusCode::CREATED
    );

    let host = wait_host(&process, HOST, |host| {
        host.status == agent::HostStatus::Incompatible
    })
    .await;
    let detail = host.detail.expect("不兼容状态带说明");
    assert!(detail.contains(tag), "说明写出远端平台: {detail}");
    assert!(
        detail.contains(agent::BUILD_PLATFORM),
        "说明写出本机平台: {detail}"
    );
    assert_eq!(ssh.installs(), 0, "没有载荷就不安装");
}

/// 不认识的远端系统同样 `incompatible`，说明保留 `uname` 的原文。
#[tokio::test]
async fn an_unknown_remote_system_is_incompatible() {
    let ssh = FakeSsh::reporting("FreeBSD amd64");
    let (process, _directory, _provider) = start(&ssh, Vec::new()).await;
    assert_eq!(
        register_host(&process, HOST, TARGET).await.status(),
        StatusCode::CREATED
    );

    let host = wait_host(&process, HOST, |host| {
        host.status == agent::HostStatus::Incompatible
    })
    .await;
    let detail = host.detail.expect("不兼容状态带说明");
    assert!(detail.contains("FreeBSD"), "{detail}");
    assert!(
        detail.contains(agent::BUILD_PLATFORM),
        "说明写出本机平台: {detail}"
    );
}

/// 规格 user story R-12、R-15：ssh 失败时 host 是 `unreachable`，说明是远端标准错误；
/// 修好 ssh 后手动重连立即成功，安装与随后的连接都不需要再做一次。
#[tokio::test]
async fn ssh_failure_is_unreachable_and_reconnect_installs_on_demand() {
    let ssh = FakeSsh::failing(1, "Permission denied (publickey).");
    let (process, _directory, _provider) = start(&ssh, Vec::new()).await;
    assert_eq!(
        register_host(&process, HOST, TARGET).await.status(),
        StatusCode::CREATED
    );

    wait_host(&process, HOST, |host| {
        host.detail.as_deref() == Some("Permission denied (publickey).")
    })
    .await;
    assert_eq!(
        installed_build(process.database(), HOST).await,
        None,
        "没连上就没有安装"
    );
    assert_eq!(ssh.installs(), 0, "失败时从未执行安装脚本");

    ssh.repair();
    reconnect(&process, HOST).await;
    wait_host(&process, HOST, |host| host.status.is_ok()).await;
    assert_eq!(
        installed_build(process.database(), HOST).await.as_deref(),
        Some(agent::BUILD_ID)
    );

    let installs = ssh.installs();
    assert!(installs >= 1, "重连时安装");
    reconnect(&process, HOST).await;
    wait_host(&process, HOST, |host| host.status.is_ok()).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(ssh.installs(), installs, "构建没变时不重复安装");
}

/// 规格 user story R-8、R-14：断链后自动按退避重连，不需要用户动手。
#[tokio::test]
async fn failed_connections_are_retried_with_backoff() {
    let ssh = FakeSsh::recovering(2);
    let (process, _directory, _provider) = start(&ssh, Vec::new()).await;
    assert_eq!(
        register_host(&process, HOST, TARGET).await.status(),
        StatusCode::CREATED
    );

    let host = wait_host(&process, HOST, |host| host.status.is_ok()).await;
    assert!(host.detail.is_none(), "重试成功后说明清空");
}

/// 规格 user story R-28、R-35：工具调用经 ssh link 在远端执行，文件副作用落在 workspace 里。
#[tokio::test]
async fn a_tool_call_runs_through_the_ssh_link() {
    let ssh = FakeSsh::working();
    let (process, directory, _provider) = start(
        &ssh,
        vec![
            Script::ToolCall {
                id: "call_ssh".into(),
                name: "bash".into(),
                arguments: r#"{"command":"printf ssh > through-ssh.txt"}"#.into(),
            },
            Script::Text("完成".into()),
        ],
    )
    .await;
    assert_eq!(
        register_host(&process, HOST, TARGET).await.status(),
        StatusCode::CREATED
    );
    wait_host(&process, HOST, |host| host.status.is_ok()).await;

    let workspace = directory.path().join("remote-workspace");
    std::fs::create_dir(&workspace).expect("创建远端 workspace");
    let path = std::fs::canonicalize(&workspace).expect("规范化 workspace");
    let response = reqwest::Client::new()
        .post(process.url("/api/workspaces"))
        .json(&serde_json::json!({ "host": HOST, "path": path }))
        .send()
        .await
        .expect("注册 workspace");
    assert_eq!(
        response.status(),
        StatusCode::CREATED,
        "workspace 经 ssh 校验"
    );

    let session = create_session_on(&process, HOST, &workspace).await;
    submit(&process, &session.id, "在远端写一个文件").await;
    let tree = wait_idle(&process, &session.id).await;

    let tools = nodes_of(&tree, agent::MessageRole::Tool);
    assert_eq!(tools.len(), 1, "一次工具调用入树为一个节点");
    let bodies: agent::MessageBodiesResponse = reqwest::Client::new()
        .post(process.url(&format!(
            "/api/sessions/{}/message-bodies",
            session.id.as_str()
        )))
        .json(&serde_json::json!({ "messageIds": [tools[0].id.get()] }))
        .send()
        .await
        .expect("读取工具正文")
        .json()
        .await
        .expect("解析工具正文");
    let (_, state) = bodies.messages[0]
        .tool_results()
        .next()
        .expect("工具结果块");
    assert!(
        matches!(
            state,
            agent::ToolResultState::Finished {
                outcome: agent::ToolOutcome::Succeeded { .. }
            }
        ),
        "工具经 ssh 成功: {state:?}"
    );
    assert_eq!(
        std::fs::read_to_string(path.join("through-ssh.txt")).expect("读取工具写入的文件"),
        "ssh"
    );
}
