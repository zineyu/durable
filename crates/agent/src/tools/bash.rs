use std::os::unix::process::ExitStatusExt;
use std::process::Stdio;
use std::time::Instant;

use durable::ToolErrorCode;
use serde::Deserialize;
use serde_json::json;
use tokio::io::{AsyncRead, AsyncReadExt};

use super::paths::Workspace;
use super::{ToolCall, ToolContext, ToolDefinition, ToolFailure, ToolLimits, parse_arguments};

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

enum Waited {
    Exited(std::process::ExitStatus),
    Interrupted(Interruption),
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
    let workspace = Workspace::new(context.workspace.as_str());
    let directory = match &arguments.cwd {
        Some(raw) => {
            let path = workspace.resolve_existing(raw)?;
            if !path.is_dir() {
                return Err(ToolFailure::new(
                    ToolErrorCode::IoError,
                    format!("cwd 不是目录: {raw}"),
                ));
            }
            path
        }
        None => workspace.root().to_path_buf(),
    };

    let mut command = tokio::process::Command::new("sh");
    command
        .arg("-c")
        .arg(&arguments.command)
        .current_dir(&directory)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .process_group(0);
    let started = Instant::now();
    let mut child = command
        .spawn()
        .map_err(|error| ToolFailure::io("启动命令", error))?;
    let mut group = ProcessGroup::new(child.id());
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ToolFailure::new(ToolErrorCode::IoError, "无法读取命令 stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| ToolFailure::new(ToolErrorCode::IoError, "无法读取命令 stderr"))?;
    let limit = limits.bash_output_bytes;
    let stdout_task = tokio::spawn(drain(stdout, limit));
    let stderr_task = tokio::spawn(drain(stderr, limit));

    let waited = tokio::select! {
        biased;
        _ = context.cancellation.cancelled() => Waited::Interrupted(Interruption::Cancelled),
        _ = tokio::time::sleep(limits.bash_timeout) => Waited::Interrupted(Interruption::TimedOut),
        status = child.wait() => Waited::Exited(
            status.map_err(|error| ToolFailure::io("等待命令结束", error))?,
        ),
    };
    let status = match waited {
        Waited::Exited(status) => {
            group.disarm();
            status
        }
        Waited::Interrupted(reason) => {
            group.kill();
            child
                .wait()
                .await
                .map_err(|error| ToolFailure::io("等待命令结束", error))?;
            let message = match reason {
                Interruption::Cancelled => "命令被取消".to_owned(),
                Interruption::TimedOut => {
                    format!("命令超过 {} ms 未结束", limits.bash_timeout.as_millis())
                }
            };
            let code = match reason {
                Interruption::Cancelled => ToolErrorCode::Cancelled,
                Interruption::TimedOut => ToolErrorCode::BashTimeout,
            };
            return Err(ToolFailure::new(code, message));
        }
    };

    let (stdout, stdout_total) = join_output(stdout_task).await?;
    let (stderr, stderr_total) = join_output(stderr_task).await?;
    let total = stdout_total + stderr_total;
    let mut output = stdout;
    output.extend_from_slice(&stderr);
    let truncated = total > limit as u64;
    if truncated {
        output.truncate(limit.min(output.len()));
    }

    let code = match (status.code(), status.signal()) {
        (Some(code), _) => code.to_string(),
        (None, Some(signal)) => format!("被信号 {signal} 终止"),
        (None, None) => "未知".to_owned(),
    };
    let mut content = format!(
        "退出码: {code}\n耗时: {} ms\n",
        started.elapsed().as_millis()
    );
    if truncated {
        content.push_str(&format!(
            "输出已截断：保留前 {limit} 字节，共 {total} 字节\n"
        ));
    }
    content.push_str("输出:\n");
    content.push_str(&String::from_utf8_lossy(&output));
    Ok(content)
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
        kept.extend_from_slice(&buffer[..read.min(room)]);
    }
}

async fn join_output(
    task: tokio::task::JoinHandle<std::io::Result<(Vec<u8>, u64)>>,
) -> Result<(Vec<u8>, u64), ToolFailure> {
    let captured = task.await.map_err(|error| {
        ToolFailure::new(ToolErrorCode::IoError, format!("读取命令输出失败: {error}"))
    })?;
    captured.map_err(|error| ToolFailure::io("读取命令输出", error))
}

/// 命令进程组。被丢弃时杀掉整个进程组，因此取消导致的 future 丢弃也会清理子进程。
struct ProcessGroup(Option<u32>);

impl ProcessGroup {
    fn new(pid: Option<u32>) -> Self {
        Self(pid)
    }

    fn kill(&self) {
        let Some(pid) = self.0 else {
            return;
        };
        // 子进程以自身 pid 作为进程组 id 启动，因此负 pid 命中整个进程组。
        unsafe {
            libc::kill(-(pid as libc::pid_t), libc::SIGKILL);
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
