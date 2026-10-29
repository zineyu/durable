//! agent 进程入口：解析参数、准备目录与日志、登记 host 与 workspace、启动 HTTP 服务。
//! 路由与状态装配在 `lib`，host 的 link 与状态在 `lib::hosts`，帧与工具调用在 `lib::link`。

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use clap::Parser;
use runtime::dirs;
use runtime::logging::{LogConfig, init};
use tracing::info;

/// 覆盖 executor 二进制位置的环境变量。
const EXECUTOR_ENV: &str = "DURABLE_EXECUTOR";

/// executor 与 agent 同目录发布时的文件名。
const EXECUTOR_FILE: &str = "executor";

#[derive(Debug, Parser)]
struct Arguments {
    /// HTTP 监听地址。
    #[arg(long, default_value = agent::DEFAULT_LISTEN)]
    listen: String,
    /// SQLite 路径。省略时使用标准数据目录。
    #[arg(long)]
    database: Option<PathBuf>,
    /// 日志目录。省略时使用标准日志目录。
    #[arg(long = "log-dir")]
    log_dir: Option<PathBuf>,
    /// 启动时登记的 workspace，写作 `<host>:<绝对路径>`，可重复。
    #[arg(long = "workspace")]
    workspaces: Vec<String>,
    /// 启动时登记的 ssh host，写作 `<name>=<ssh-target>`，可重复。
    #[arg(long = "host")]
    hosts: Vec<String>,
}

/// 已确定的启动路径与日志参数。
struct Prepared {
    address: SocketAddr,
    database: PathBuf,
    log: LogConfig,
}

#[tokio::main]
async fn main() -> ExitCode {
    let arguments = Arguments::parse();
    let prepared = match prepare(&arguments) {
        Ok(prepared) => prepared,
        Err(error) => {
            report_startup_failure(&error);
            return ExitCode::FAILURE;
        }
    };
    let _log = match init(&prepared.log) {
        Ok(guard) => guard,
        Err(error) => {
            report_startup_failure(&error);
            return ExitCode::FAILURE;
        }
    };
    match run(arguments, prepared).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!("启动失败: {error}");
            ExitCode::FAILURE
        }
    }
}

/// 启动早期失败时日志尚未初始化，只能写标准错误。
#[expect(
    clippy::print_stderr,
    reason = "日志尚未初始化，启动失败只能写标准错误。"
)]
fn report_startup_failure(error: &dyn std::fmt::Display) {
    eprintln!("{error}");
}

/// 监听地址写到标准输出，是启动协议的一部分：调用方据此得知就绪端口。
#[expect(
    clippy::print_stdout,
    reason = "监听地址是启动协议的一部分，调用方从标准输出读取。"
)]
fn report_listen_address(address: SocketAddr) {
    println!("agent listening on http://{address}");
}

fn prepare(arguments: &Arguments) -> Result<Prepared, Box<dyn std::error::Error>> {
    let address = agent::parse_listen_address(&arguments.listen)?;
    let paths = dirs::startup_paths(
        arguments.database.as_deref(),
        arguments.log_dir.as_deref(),
        agent::DEFAULT_DATABASE_FILE,
    )?;
    Ok(Prepared {
        address,
        database: paths.database,
        log: LogConfig::new(paths.log_directory, agent::LOG_FILE_PREFIX),
    })
}

async fn run(arguments: Arguments, prepared: Prepared) -> Result<(), Box<dyn std::error::Error>> {
    let model = agent::model_config_from_env()?;
    let engine = agent::engine_config_from_env()?;
    let workspaces = resolve_workspaces(&arguments.workspaces)?;
    let hosts = resolve_hosts(&arguments.hosts)?;
    let limits = agent::ToolLimits::default();
    let store = Arc::new(agent::Store::open(&prepared.database).await?);
    for (name, target) in hosts {
        if store.host(&name).await?.is_none() {
            store
                .register_host(name, agent::HostKind::Ssh, Some(target))
                .await?;
        }
    }
    let executor = executor_binary()?;
    let hosts = agent::HostRegistry::start(Arc::clone(&store), executor.clone(), limits).await?;
    register_startup_workspaces(&store, &hosts, &workspaces).await?;
    let provider = Arc::new(agent::OpenAiProvider::new(&model));
    let model_name = model.model_name.clone();
    let tools = Arc::new(agent::RemoteTools::new(Arc::clone(&hosts)));
    let state = agent::AgentState::start(store, hosts, model_name, provider, tools, engine).await?;
    info!(
        executor = %executor.display(),
        database = %prepared.database.display(),
        log_directory = %prepared.log.directory.display(),
        log_filter = %prepared.log.filter,
        workspaces = workspaces.len(),
        model = %model.model_name,
        "agent 启动"
    );
    let listener = tokio::net::TcpListener::bind(prepared.address).await?;
    let bound = listener.local_addr()?;
    report_listen_address(bound);
    info!(listen = %bound, "agent 开始监听");
    agent::serve(listener, state).await?;
    Ok(())
}

/// 定位 executor 二进制：`DURABLE_EXECUTOR` 优先，否则取与 agent 同目录的同名文件。
///
/// 失败：环境变量缺失且自身路径没有父目录，或该文件不存在。
fn executor_binary() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let path = if let Some(value) = std::env::var_os(EXECUTOR_ENV) {
        PathBuf::from(value)
    } else {
        let current = std::env::current_exe()?;
        let directory = current.parent().ok_or_else(|| {
            std::io::Error::other(format!("无法确定 agent 所在目录: {}", current.display()))
        })?;
        directory.join(EXECUTOR_FILE)
    };
    if !path.is_file() {
        return Err(format!(
            "找不到 executor 二进制: {}。它与 agent 同目录发布；也可用 {EXECUTOR_ENV} 指定路径。",
            path.display()
        )
        .into());
    }
    Ok(path)
}

/// 把 `--workspace <host>:<path>` 解析为 host 与路径这一对。
///
/// 失败：缺少冒号、host 名不合法、路径不是绝对路径。
fn resolve_workspaces(
    entries: &[String],
) -> Result<Vec<(agent::HostName, agent::WorkspacePath)>, Box<dyn std::error::Error>> {
    entries
        .iter()
        .map(|entry| {
            let (host, path) = entry
                .split_once(':')
                .ok_or_else(|| format!("--workspace 需要写成 <host>:<绝对路径>，实际是 {entry}"))?;
            Ok((
                agent::HostName::parse(host)?,
                agent::WorkspacePath::parse(path)?,
            ))
        })
        .collect()
}

/// 把 `--host <name>=<ssh-target>` 解析为 host 名与目标字符串。
///
/// 失败：缺少等号、host 名不合法、目标为空。
fn resolve_hosts(
    entries: &[String],
) -> Result<Vec<(agent::HostName, String)>, Box<dyn std::error::Error>> {
    entries
        .iter()
        .map(|entry| {
            let (name, target) = entry
                .split_once('=')
                .ok_or_else(|| format!("--host 需要写成 <name>=<ssh-target>，实际是 {entry}"))?;
            let target = target.trim();
            if target.is_empty() {
                return Err(format!("--host 的 SSH 目标不能为空: {entry}").into());
            }
            Ok((agent::HostName::parse(name)?, target.to_owned()))
        })
        .collect()
}

/// 登记启动参数给出的 workspace：host 必须已登记，路径由 host 判定。
///
/// host 当前不可用时跳过并留下告警：已登记的 workspace 仍可用，不必让整个进程起不来。
async fn register_startup_workspaces(
    store: &agent::Store,
    hosts: &agent::HostRegistry,
    workspaces: &[(agent::HostName, agent::WorkspacePath)],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut accepted: Vec<(agent::HostName, agent::WorkspacePath)> = Vec::new();
    for (host, path) in workspaces {
        if store.host(host).await?.is_none() {
            return Err(format!(
                "host 未登记: {host}。先用 --host 登记，或核对 --workspace 的 host 名。"
            )
            .into());
        }
        match hosts.check_workspace(host, path.as_str()).await {
            Some(Ok(canonical)) => {
                accepted.push((host.clone(), agent::WorkspacePath::parse(&canonical)?));
            }
            Some(Err(error)) => {
                return Err(format!("workspace 路径不可用 {}：{error}", path.as_str()).into());
            }
            None => {
                tracing::warn!(host = %host, path = %path.as_str(), "host 当前不可用，跳过启动参数里的 workspace");
            }
        }
    }
    store.register_initial_workspaces(&accepted).await?;
    Ok(())
}
