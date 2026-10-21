//! `bash` 工具：进程组执行、输出截断与超时或取消处理。

use std::fmt::Write;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::time::{Duration, Instant};

use durable::ToolErrorCode;
use serde::Deserialize;
use serde_json::json;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::{Child, Command};
use tokio::task::JoinHandle;

use super::paths::Workspace;
use super::{ToolCall, ToolContext, ToolDefinition, ToolFailure, ToolLimits, parse_arguments};

use crate::cancel::Cancellation;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Arguments {
    pub command: String,
    #[serde(default)]
    pub cwd: Option<String>,
}

pub(crate) fn definition() -> ToolDefinition {
    ToolDefinition {
        name: "bash",
        description: "在 workspace 根内以 sh -c 执行命令。stdin 关闭，stdout 与 stderr 合并返回，返回退出码与耗时。",
        parameters: json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "交给 sh -c 的命令" },
                "cwd": { "type": "string", "description": "相对 workspace 根的工作目录，默认根" },
            },
            "required": ["command"],
            "additionalProperties": false,
        }),
    }
}

enum Interruption {
    Cancelled,
    TimedOut,
}

/// 执行命令并返回退出码、耗时与合并输出。
///
/// 失败：路径越界、cwd 不存在或不是目录、命令无法启动、超时、取消。
/// 副作用：运行子进程；超时或取消时杀掉整个进程组。
pub(crate) async fn run(
    call: &ToolCall,
    context: &ToolContext,
    limits: &ToolLimits,
) -> Result<String, ToolFailure> {
    let arguments: Arguments = parse_arguments(call)?;
    if arguments.command.trim().is_empty() {
        return Err(ToolFailure::new(
            ToolErrorCode::InvalidArguments,
            "command 不能为空",
        ));
    }
    let directory = resolve_directory(&arguments, context)?;
    let started = Instant::now();
    let mut child = spawn(&arguments, &directory)?;
    let mut group = ProcessGroup::new(child.id());
    let stdout = take_pipe(child.stdout.take(), "stdout")?;
    let stderr = take_pipe(child.stderr.take(), "stderr")?;
    let limit = limits.bash_output_bytes;
    let stdout_task = tokio::spawn(drain(stdout, limit));
    let stderr_task = tokio::spawn(drain(stderr, limit));
    let status = wait(&mut child, &mut group, &context.cancellation, limits).await?;
    let (output, total) = collect(stdout_task, stderr_task, limit).await?;
    render(status, started.elapsed(), &output, total, limit)
}

/// 把可选的 `cwd` 解析到 workspace 根内，缺省为根。
fn resolve_directory(arguments: &Arguments, context: &ToolContext) -> Result<PathBuf, ToolFailure> {
    let workspace = Workspace::new(context.workspace.as_str());
    let Some(raw) = &arguments.cwd else {
        return Ok(workspace.root().to_path_buf());
    };
    let path = workspace.resolve_existing(raw)?;
    if !path.is_dir() {
        return Err(ToolFailure::new(
            ToolErrorCode::IoError,
            format!("cwd 不是目录: {raw}"),
        ));
    }
    Ok(path)
}

fn spawn(arguments: &Arguments, directory: &Path) -> Result<Child, ToolFailure> {
    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg(&arguments.command)
        .current_dir(directory)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .process_group(0);
    command
        .spawn()
        .map_err(|error| ToolFailure::io("启动命令", &error))
}

fn take_pipe<F>(pipe: Option<F>, name: &str) -> Result<F, ToolFailure> {
    pipe.ok_or_else(|| ToolFailure::new(ToolErrorCode::IoError, format!("无法读取命令 {name}")))
}

/// 等待命令结束。超时或取消时杀掉进程组并返回失败。
async fn wait(
    child: &mut Child,
    group: &mut ProcessGroup,
    cancellation: &Cancellation,
    limits: &ToolLimits,
) -> Result<ExitStatus, ToolFailure> {
    let waited = tokio::select! {
        biased;
        () = cancellation.cancelled() => Err(Interruption::Cancelled),
        () = tokio::time::sleep(limits.bash_timeout) => Err(Interruption::TimedOut),
        status = child.wait() => Ok(status.map_err(|error| ToolFailure::io("等待命令结束", &error))?),
    };
    let reason = match waited {
        Ok(status) => {
            group.disarm();
            return Ok(status);
        }
        Err(reason) => reason,
    };
    group.kill();
    child
        .wait()
        .await
        .map_err(|error| ToolFailure::io("等待命令结束", &error))?;
    let message = match reason {
        Interruption::Cancelled => "命令被取消".to_owned(),
        Interruption::TimedOut => format!("命令超过 {} ms 未结束", limits.bash_timeout.as_millis()),
    };
    let failure_code = match reason {
        Interruption::Cancelled => ToolErrorCode::Cancelled,
        Interruption::TimedOut => ToolErrorCode::BashTimeout,
    };
    Err(ToolFailure::new(failure_code, message))
}

/// 合并 stdout 与 stderr，并按上限截断。返回合并结果与截断前的总字节数。
async fn collect(
    stdout_task: JoinHandle<std::io::Result<(Vec<u8>, u64)>>,
    stderr_task: JoinHandle<std::io::Result<(Vec<u8>, u64)>>,
    limit: usize,
) -> Result<(Vec<u8>, u64), ToolFailure> {
    let (stdout, stdout_total) = join_output(stdout_task).await?;
    let (stderr, stderr_total) = join_output(stderr_task).await?;
    let total = stdout_total + stderr_total;
    let mut output = stdout;
    output.extend_from_slice(&stderr);
    if total > limit as u64 {
        output.truncate(limit.min(output.len()));
    }
    Ok((output, total))
}

/// 组装工具结果：退出码、耗时、截断说明与输出正文。
fn render(
    status: ExitStatus,
    elapsed: Duration,
    output: &[u8],
    total: u64,
    limit: usize,
) -> Result<String, ToolFailure> {
    let exit_status = match (status.code(), status.signal()) {
        (Some(code), _) => code.to_string(),
        (None, Some(signal)) => format!("被信号 {signal} 终止"),
        (None, None) => "未知".to_owned(),
    };
    let mut result_text = format!("退出码: {exit_status}\n耗时: {} ms\n", elapsed.as_millis());
    if total > limit as u64 {
        writeln!(
            result_text,
            "输出已截断：保留前 {limit} 字节，共 {total} 字节"
        )
        .map_err(|error| {
            ToolFailure::new(ToolErrorCode::IoError, format!("写入命令输出: {error}"))
        })?;
    }
    result_text.push_str("输出:\n");
    result_text.push_str(&String::from_utf8_lossy(output));
    Ok(result_text)
}

async fn drain(mut pipe: impl AsyncRead + Unpin, limit: usize) -> std::io::Result<(Vec<u8>, u64)> {
    let mut kept = Vec::new();
    let mut total = 0u64;
    let mut buffer = [0u8; 8192];
    loop {
        let read = pipe.read(&mut buffer).await?;
        if read == 0 {
            return Ok((kept, total));
        }
        total += read as u64;
        let room = limit.saturating_sub(kept.len());
        let take = read.min(room).min(buffer.len());
        kept.extend(buffer.iter().take(take).copied());
    }
}

async fn join_output(
    task: JoinHandle<std::io::Result<(Vec<u8>, u64)>>,
) -> Result<(Vec<u8>, u64), ToolFailure> {
    let captured = task.await.map_err(|error| {
        ToolFailure::new(ToolErrorCode::IoError, format!("读取命令输出失败: {error}"))
    })?;
    captured.map_err(|error| ToolFailure::io("读取命令输出", &error))
}

/// 命令进程组。被丢弃时杀掉整个进程组，因此取消导致的 future 丢弃也会清理子进程。
struct ProcessGroup(Option<u32>);

impl ProcessGroup {
    fn new(pid: Option<u32>) -> Self {
        Self(pid)
    }

    #[expect(
        unsafe_code,
        reason = "终止子进程只能调用 libc::kill；标准库没有杀进程组的接口。"
    )]
    fn kill(&self) {
        let Some(pid) = self.0 else {
            return;
        };
        let Ok(target) = libc::pid_t::try_from(pid) else {
            return;
        };
        // SAFETY: 子进程以自身 pid 作为进程组 id 启动，因此负 pid 命中整个进程组。
        // 该进程组由本模块创建，pid 来自 Child::id，调用期间不失效。
        unsafe {
            libc::kill(-target, libc::SIGKILL);
        }
    }

    fn disarm(&mut self) {
        self.0 = None;
    }
}

impl Drop for ProcessGroup {
    fn drop(&mut self) {
        self.kill();
    }
}
