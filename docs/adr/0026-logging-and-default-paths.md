# ADR 0026: 日志与默认路径

- 状态：已接受
- 日期：2026-10-20

## 背景

`agent` 与 `hub` 原先要求 `--database`，且没有日志系统。诊断信息只经零散的 `eprintln!` 输出到标准错误，进程退出后不可追溯。缺少参数时用户必须自行决定数据库位置，数据库容易散落在仓库或临时目录。

## 决策

**新增 `crates/runtime`（包名 `runtime`），承载进程级支撑**：标准目录解析与日志初始化。`agent`、`hub` 与 `durable` 内核都不管平台目录；内核只发 `tracing` 事件。

**默认目录跟随平台惯例**：

| 平台 | 数据目录 | 日志目录 |
| --- | --- | --- |
| macOS | `~/Library/Application Support/durable` | `~/Library/Logs/durable` |
| Linux | `$XDG_DATA_HOME/durable`，默认 `~/.local/share/durable` | `$XDG_STATE_HOME/durable/log`，默认 `~/.local/state/durable/log` |
| Windows | `%LOCALAPPDATA%\durable` | `%LOCALAPPDATA%\durable\logs` |

XDG 变量为空时按未设置处理。目录解析读取的环境变量取自一个显式结构体，因此解析结果可以脱离进程环境测试。

**`--database` 与 `--log-dir` 均可省略**：省略数据库时使用数据目录下的 `agent.sqlite` 或 `hub.sqlite`；省略日志目录时使用标准日志目录。给出显式路径时不需要 `HOME` 等基础变量。

**日志双路输出**：标准错误用 `tracing-subscriber` 的 pretty 格式，供人阅读，终端中带颜色；文件按天轮转，每行一条 JSON 记录，供 `jq` 检索与聚合。文件名为 `<prefix>.<YYYY-MM-DD>.log`，保留最近 14 个。

**过滤由 `DURABLE_LOG` 控制**，指令语法同 `RUST_LOG`，默认 `info`。过滤器无效时回退到默认过滤器并在标准错误给出警告：它是可选的诊断开关，拼错的指令不应阻断启动。日志目录或日志文件建立失败会终止启动：静默丢失日志比启动失败更难诊断。文件写入是非阻塞且允许丢弃的：日志 I/O 不得阻塞 agent 循环。

**日志点覆盖生命周期**：启动路径与监听地址、run 开始与结束、工具失败、节点状态变化、节点注册与增删、存储打开、启动恢复。正常路径用 `info` 或 `debug`，需要人工关注的路径用 `warn` 或 `error`。

## 后果

- 无参数启动即可用：数据库与日志落在标准位置。
- 测试必须显式指定 `--log-dir`，否则会写入用户的标准日志目录。测试辅助统一从数据库路径派生临时日志目录。
- 同一机器的多个 node 必须各自指定 `--database` 与 `--log-dir`：默认值只服务单 node。
- 日志目录、数据库默认路径与日志格式属于进程行为，因此不进 `durable` 内核。
- 配置文件与远程日志导出仍不在规格内。
