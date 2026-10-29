//! 测试用的假 `ssh`：一个独立 shell 脚本，放在 `PATH` 首位替换系统的 ssh。
//! 它把每次调用的 argv 记进日志，再把目标之后的命令交给本机的 `sh` 执行。
//! 真实 `agent`、`executor` 与帧协议都不变；安装与 link 因此真的走一遍。

#![expect(
    dead_code,
    clippy::expect_used,
    reason = "测试脚手架用 expect 直接报告夹具失败；本模块被每个测试目标各编译一次，未被该目标用到的条目属正常情况。"
)]

use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

/// 日志里每次调用的分隔行。
const SEPARATOR: &str = "-- invocation";

/// 一次假 `ssh` 调用。
#[derive(Clone)]
pub(crate) struct Invocation {
    pub(crate) arguments: Vec<String>,
}

impl Invocation {
    /// 这次调用是不是安装：安装脚本里有 `cat > …executor-`。
    fn is_install(&self) -> bool {
        self.arguments
            .iter()
            .any(|argument| argument.contains("cat >") && argument.contains("executor-"))
    }
}

/// 假 `ssh` 与它的状态目录：日志、失败计数与远端的 `XDG_DATA_HOME`。
pub(crate) struct FakeSsh {
    directory: TempDir,
    script: PathBuf,
    log: PathBuf,
    counter: PathBuf,
    data: PathBuf,
}

impl FakeSsh {
    /// 一次都不失败的假 ssh：远端命令在本机真实执行。
    pub(crate) fn working() -> Self {
        Self::new(0, 1, "", "")
    }

    /// 每次都失败的假 ssh：写一行标准错误并以 `code` 退出。
    pub(crate) fn failing(code: i32, message: &str) -> Self {
        Self::new(1_000_000, code, message, "")
    }

    /// 前 `failures` 次失败、之后恢复的假 ssh：用于验证自动退避重连。
    pub(crate) fn recovering(failures: usize) -> Self {
        Self::new(failures, 1, "Connection refused", "")
    }

    /// 远端 `uname -sm` 报告指定平台，其余命令照常在本机执行。
    pub(crate) fn reporting(uname: &str) -> Self {
        Self::new(0, 1, "", uname)
    }

    fn new(failures: usize, code: i32, message: &str, uname: &str) -> Self {
        let directory = tempfile::tempdir().expect("创建假 ssh 目录");
        let script = directory.path().join("ssh");
        let log = directory.path().join("ssh.log");
        let counter = directory.path().join("count");
        let data = directory.path().join("data");
        let fake = Self {
            directory,
            script,
            log,
            counter,
            data,
        };
        fake.write(failures, code, message, uname);
        fake
    }

    /// 换成一次都不失败的假 ssh：用于「修好 ssh 再重连」。
    pub(crate) fn repair(&self) {
        self.write(0, 1, "", "");
    }

    fn write(&self, failures: usize, code: i32, message: &str, uname: &str) {
        let script = format!(
            r#"#!/bin/sh
log='{log}'
count='{counter}'
data='{data}'
{{
  printf '%s\n' '{separator}'
  for arg in "$@"; do printf '%s\n' "$arg"; done
}} >> "$log"
n=$(cat "$count" 2>/dev/null || printf '0')
n=$((n + 1))
printf '%s\n' "$n" > "$count"
if [ "$n" -le {failures} ]; then
  printf '%s\n' '{message}' >&2
  exit {code}
fi
shift
if [ -n '{uname}' ]; then
  case "$*" in
    *"uname -sm"*) printf '%s\n' '{uname}'; exit 0 ;;
  esac
fi
XDG_DATA_HOME="$data" exec sh -c "$*"
"#,
            log = self.log.display(),
            counter = self.counter.display(),
            data = self.data.display(),
            separator = SEPARATOR,
            message = message.replace('\'', r"'\''"),
            uname = uname,
        );
        std::fs::write(&self.script, script).expect("写入假 ssh");
        std::fs::set_permissions(&self.script, std::fs::Permissions::from_mode(0o755))
            .expect("给假 ssh 加可执行位");
    }

    /// 把假 ssh 放到 `PATH` 的首位，其余路径保持不变。
    pub(crate) fn path_environment(&self) -> OsString {
        let existing = std::env::var_os("PATH").unwrap_or_default();
        let mut paths = vec![self.directory.path().to_path_buf()];
        paths.extend(std::env::split_paths(&existing));
        std::env::join_paths(paths).expect("拼接 PATH")
    }

    /// 全部调用，按发生顺序。
    pub(crate) fn invocations(&self) -> Vec<Invocation> {
        let Ok(log) = std::fs::read_to_string(&self.log) else {
            return Vec::new();
        };
        let mut invocations: Vec<Invocation> = Vec::new();
        for line in log.lines() {
            if line == SEPARATOR {
                invocations.push(Invocation {
                    arguments: Vec::new(),
                });
            } else if let Some(current) = invocations.last_mut() {
                current.arguments.push(line.to_owned());
            }
        }
        invocations
    }

    /// 等到出现满足条件的调用。`Command::spawn` 返回时子进程可能还没写出日志，
    /// 因此对「刚拉起的 link」不能读一次就算。
    pub(crate) async fn wait_for_invocation(
        &self,
        matches: impl Fn(&Invocation) -> bool,
    ) -> Invocation {
        for _ in 0..500 {
            if let Some(invocation) = self.invocations().into_iter().find(|i| matches(i)) {
                return invocation;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("假 ssh 没有在预期时间内收到匹配的调用");
    }

    /// 执行过安装脚本的次数。
    pub(crate) fn installs(&self) -> usize {
        self.invocations()
            .iter()
            .filter(|invocation| invocation.is_install())
            .count()
    }

    /// 远端的 `XDG_DATA_HOME`：安装与启动都落在它下面。
    pub(crate) fn data_directory(&self) -> &Path {
        &self.data
    }
}
