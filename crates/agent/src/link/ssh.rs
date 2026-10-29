//! ssh 链路的准备：探测远端平台、选择 executor 载荷、安装到远端数据目录。
//! 命令组装在 `transport`，link 的读写与请求配对在父模块，host 三态在 `crate::hosts`。
//!
//! 这里只回答「远端能不能跑这次构建的 executor」：平台映射不到载荷就是 `incompatible`，
//! ssh 本身跑不通或安装失败就是 `unreachable`。状态由 host registry 落成事实。

use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};

use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use super::closed::INCOMPATIBLE_EXIT_CODE;
use super::transport::{
    BUILD_ID, BUILD_PLATFORM, PAYLOAD_ENV_PREFIX, install_command, probe_command,
};

/// 建立 ssh 链路的两种失败。调用方把它映射成 host 的 `incompatible` 与 `unreachable`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SetupFailure {
    /// 远端平台没有载荷，或帧协议版本不符。
    Incompatible(String),
    /// ssh 跑不通、安装失败或载荷读不到。说明里保留远端原文。
    Unreachable(String),
}

/// 准备远端：`installed_build` 与本次构建不符时探测平台并安装。
///
/// 返回是否安装了新载荷：调用方据此把构建 id 记进 host 行。
///
/// # Errors
///
/// - [`SetupFailure::Incompatible`]：远端平台没有可用载荷，或两侧帧协议版本不符。
/// - [`SetupFailure::Unreachable`]：ssh 无法启动、远端命令失败或载荷文件读不到。
pub(crate) async fn prepare(
    target: &str,
    installed_build: Option<&str>,
    local_executor: &Path,
) -> Result<bool, SetupFailure> {
    if installed_build == Some(BUILD_ID) {
        return Ok(false);
    }
    let probe = run_capture(probe_command(target)).await?;
    let platform = remote_platform(&probe)?;
    let payload = payload_for(&platform, local_executor).await?;
    install(target, &payload).await?;
    Ok(true)
}

/// 远端 `uname -sm` 的输出 → 平台标签。不认识的组合说明两侧都看到了什么。
fn remote_platform(uname: &str) -> Result<String, SetupFailure> {
    let mut parts = uname.split_whitespace();
    let system = parts.next().unwrap_or_default();
    let arch = parts.next().unwrap_or_default();
    let system = match system {
        "Linux" => "linux",
        "Darwin" => "darwin",
        other => {
            return Err(SetupFailure::Incompatible(format!(
                "不支持远端系统 {other:?}（本机构建 {BUILD_PLATFORM}）"
            )));
        }
    };
    let arch = match arch {
        "x86_64" | "amd64" => "x86_64",
        "aarch64" | "arm64" => "aarch64",
        other => {
            return Err(SetupFailure::Incompatible(format!(
                "不支持远端架构 {other:?}（本机构建 {BUILD_PLATFORM}）"
            )));
        }
    };
    Ok(format!("{system}-{arch}"))
}

/// 远端平台的 executor 载荷：环境变量优先，否则仅当平台与本地构建一致时用同目录的二进制。
async fn payload_for(platform: &str, local_executor: &Path) -> Result<Vec<u8>, SetupFailure> {
    let variable = format!("{PAYLOAD_ENV_PREFIX}{platform}");
    let path = match std::env::var_os(&variable) {
        Some(path) => PathBuf::from(path),
        None if platform == BUILD_PLATFORM => local_executor.to_path_buf(),
        None => {
            return Err(SetupFailure::Incompatible(format!(
                "远端平台 {platform} 没有 executor 载荷（本机构建 {BUILD_PLATFORM}）：设置 {variable} 指向交叉构建的二进制"
            )));
        }
    };
    tokio::fs::read(&path).await.map_err(|error| {
        SetupFailure::Unreachable(format!(
            "读取 executor 载荷失败 {}：{error}",
            path.display()
        ))
    })
}

/// 一次 `ssh` 调用并收下它的标准输出。退出码 3 是版本不符，其余非零是远端失败。
async fn run_capture(mut command: Command) -> Result<String, SetupFailure> {
    let output = command
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|error| SetupFailure::Unreachable(format!("启动 ssh 失败：{error}")))?;
    classify(output.status, &output.stderr)?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// 把载荷写到远端。载荷走标准输入，不出现在命令行与日志里。
async fn install(target: &str, payload: &[u8]) -> Result<(), SetupFailure> {
    let mut child = install_command(target)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| SetupFailure::Unreachable(format!("启动 ssh 失败：{error}")))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| SetupFailure::Unreachable("ssh 没有标准输入".to_owned()))?;
    if let Err(error) = stdin.write_all(payload).await {
        tracing::debug!(error = %error, "向 ssh 写载荷中断，以退出码与标准错误判定");
    }
    drop(stdin);
    let output = child
        .wait_with_output()
        .await
        .map_err(|error| SetupFailure::Unreachable(format!("等待 ssh 失败：{error}")))?;
    classify(output.status, &output.stderr)
}

/// 按退出状态与标准错误归类一次远端命令。
fn classify(status: ExitStatus, stderr: &[u8]) -> Result<(), SetupFailure> {
    if status.success() {
        return Ok(());
    }
    let tail = String::from_utf8_lossy(stderr).trim().to_owned();
    let detail = if tail.is_empty() {
        match status.code() {
            Some(code) => format!("ssh 退出码 {code}"),
            None => "ssh 被信号终止".to_owned(),
        }
    } else {
        tail
    };
    if status.code() == Some(INCOMPATIBLE_EXIT_CODE) {
        return Err(SetupFailure::Incompatible(format!(
            "帧协议版本不符（{detail}）"
        )));
    }
    Err(SetupFailure::Unreachable(detail))
}

#[cfg(test)]
mod tests {
    use super::{SetupFailure, classify, remote_platform};
    use std::os::unix::process::ExitStatusExt;
    use std::process::ExitStatus;

    #[test]
    fn maps_the_supported_uname_combinations() {
        for (uname, platform) in [
            ("Linux x86_64", "linux-x86_64"),
            ("Linux aarch64", "linux-aarch64"),
            ("Linux arm64", "linux-aarch64"),
            ("Darwin arm64", "darwin-aarch64"),
            ("Darwin x86_64", "darwin-x86_64"),
        ] {
            assert_eq!(
                remote_platform(uname).expect("支持的平台"),
                platform,
                "{uname}"
            );
        }
    }

    #[test]
    fn unknown_remote_systems_are_incompatible_and_name_both_sides() {
        let failure = remote_platform("FreeBSD amd64").expect_err("不支持的系统");
        let SetupFailure::Incompatible(detail) = failure else {
            panic!("不支持的系统应是 incompatible");
        };
        assert!(detail.contains("FreeBSD"), "{detail}");
        assert!(detail.contains(super::BUILD_PLATFORM), "{detail}");
    }

    #[test]
    fn exit_code_three_is_a_protocol_mismatch() {
        let status = ExitStatus::from_raw(3 << 8);
        let failure = classify(status, "帧协议版本不符".as_bytes()).expect_err("退出码 3");
        assert!(
            matches!(failure, SetupFailure::Incompatible(_)),
            "{failure:?}"
        );
        let SetupFailure::Incompatible(detail) = failure else {
            panic!("退出码 3 应是 incompatible");
        };
        assert!(detail.contains("帧协议版本不符"), "{detail}");
    }

    #[test]
    fn other_failures_keep_the_remote_stderr() {
        let status = ExitStatus::from_raw(1 << 8);
        let failure = classify(status, b"Permission denied (publickey).").expect_err("退出码 1");
        assert_eq!(
            failure,
            SetupFailure::Unreachable("Permission denied (publickey).".to_owned())
        );
        let bare = classify(ExitStatus::from_raw(255 << 8), b"").expect_err("没有标准错误");
        assert_eq!(bare, SetupFailure::Unreachable("ssh 退出码 255".to_owned()));
    }
}
