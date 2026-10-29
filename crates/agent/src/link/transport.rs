//! link 的两种传输：本机直接拉起 executor，ssh 经 `ssh <target> sh -c '<脚本>'`。
//! 载荷选择、探测与安装在 `ssh`；帧读写与请求配对在父模块；host 三态在 `crate::hosts`。
//!
//! 两种传输的产物都是同一个子进程：标准输入输出是 link，标准错误并入 agent 的日志。
//! 远端脚本自己解析数据目录，因此每次连接都用同一个绝对路径，不依赖远端登录 shell 的环境。

use std::path::PathBuf;
use std::process::Stdio;

use durable::ToolLimits;
use durable::link::LINK_PROTOCOL_VERSION;
use tokio::process::Command;

/// 本次构建的 id：包版本加目标三元组。远端文件名以它结尾。
pub const BUILD_ID: &str = env!("DURABLE_BUILD_ID");

/// 本次构建的平台标签，如 `linux-x86_64`。远端平台与它不符时需要对应载荷。
pub const BUILD_PLATFORM: &str = env!("DURABLE_BUILD_PLATFORM");

/// 远端数据目录：`$XDG_DATA_HOME` 优先，否则标准数据目录下的 `durable`。
const REMOTE_DATA_DIR: &str = "\"${XDG_DATA_HOME:-$HOME/.local/share}/durable\"";

/// 测试载荷环境变量与发布钩子的前缀：`DURABLE_EXECUTOR_<平台>`。
pub(crate) const PAYLOAD_ENV_PREFIX: &str = "DURABLE_EXECUTOR_";

/// link 子进程的启动方式。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Transport {
    /// 本机：直接拉起与 agent 同目录发布的 executor。
    Local { executor: PathBuf },
    /// 远端：目标字符串交给 `~/.ssh/config` 与 ssh-agent 解析。
    Ssh { target: String },
}

impl Transport {
    /// 组装 link 子进程命令。两条路径的管道与生命周期完全相同。
    #[must_use]
    pub(crate) fn command(&self, limits: ToolLimits) -> Command {
        let mut command = match self {
            Self::Local { executor } => {
                let mut command = Command::new(executor);
                command
                    .arg("--link-protocol")
                    .arg(LINK_PROTOCOL_VERSION.to_string())
                    .arg("--read-bytes")
                    .arg(limits.read_bytes.to_string())
                    .arg("--bash-output-bytes")
                    .arg(limits.bash_output_bytes.to_string())
                    .arg("--bash-timeout-ms")
                    .arg(limits.bash_timeout.as_millis().to_string());
                command
            }
            Self::Ssh { target } => ssh_command(target, &launch_script(limits)),
        };
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        command
    }
}

/// 远端启动脚本：解析数据目录后 `exec` 已安装的 executor。
///
/// 工具上限随启动参数交给 executor，因此两侧看到的截断与超时一致。
#[must_use]
pub(crate) fn launch_script(limits: ToolLimits) -> String {
    format!(
        "d={REMOTE_DATA_DIR}; exec \"$d/executor-{BUILD_ID}\" --link-protocol {LINK_PROTOCOL_VERSION} \
         --read-bytes {} --bash-output-bytes {} --bash-timeout-ms {}",
        limits.read_bytes,
        limits.bash_output_bytes,
        limits.bash_timeout.as_millis()
    )
}

/// 远端安装脚本：载荷从标准输入写入数据目录，加可执行位。
#[must_use]
pub(crate) fn install_script() -> String {
    format!(
        "d={REMOTE_DATA_DIR}; mkdir -p \"$d\" && cat > \"$d/executor-{BUILD_ID}\" \
         && chmod 755 \"$d/executor-{BUILD_ID}\""
    )
}

/// 探测远端平台。只在安装前用一次。
pub(crate) fn probe_command(target: &str) -> Command {
    ssh_command(target, "uname -sm")
}

/// 把 executor 载荷写到远端。载荷走标准输入，不出现在命令行里。
pub(crate) fn install_command(target: &str) -> Command {
    ssh_command(target, &install_script())
}

/// `ssh <target> sh -c '<脚本>'`：目标是一个 argv，脚本是另一个，用户输入无法注入额外单词。
fn ssh_command(target: &str, script: &str) -> Command {
    let mut command = Command::new("ssh");
    command
        .arg(target)
        .arg("sh")
        .arg("-c")
        .arg(shell_quote(script));
    command
}

/// 把脚本包成一个 shell 单词：外层单引号，内部的单引号写成 `'\''`。
fn shell_quote(script: &str) -> String {
    format!("'{}'", script.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::{Transport, install_script, launch_script, shell_quote};
    use durable::ToolLimits;

    #[test]
    fn launch_script_resolves_the_data_directory_and_carries_the_limits() {
        let script = launch_script(ToolLimits::default());
        assert!(
            script.contains(r#"d="${XDG_DATA_HOME:-$HOME/.local/share}/durable""#),
            "{script}"
        );
        assert!(script.contains("--link-protocol"), "{script}");
        assert!(script.contains("--bash-timeout-ms"), "{script}");
        assert!(script.contains("exec \"$d/executor-"), "{script}");
    }

    #[test]
    fn install_script_writes_the_build_named_file() {
        let script = install_script();
        assert!(script.contains(r#"mkdir -p "$d""#), "{script}");
        assert!(script.contains(r#"cat > "$d/executor-"#), "{script}");
        assert!(script.contains("chmod 755"), "{script}");
    }

    #[test]
    fn quoting_survives_embedded_single_quotes() {
        assert_eq!(shell_quote("a b"), "'a b'");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
        assert_eq!(shell_quote("$HOME"), "'$HOME'");
    }

    #[test]
    fn ssh_transport_puts_the_target_first() {
        let command = Transport::Ssh {
            target: "box".to_owned(),
        }
        .command(ToolLimits::default());
        let program = command
            .as_std()
            .get_program()
            .to_string_lossy()
            .into_owned();
        assert_eq!(program, "ssh");
        let arguments: Vec<String> = command
            .as_std()
            .get_args()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect();
        assert_eq!(arguments.first().map(String::as_str), Some("box"));
        assert_eq!(&arguments[1..3], ["sh", "-c"]);
        assert!(arguments[3].contains("--link-protocol"), "{arguments:?}");
        assert!(arguments[3].starts_with('\''), "脚本被引用成一个单词");
    }
}
