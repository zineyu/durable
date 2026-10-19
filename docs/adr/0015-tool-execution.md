# ADR 0015: 工具执行与 workspace 路径边界

- 状态：已接受
- 日期：2026-11-01

## 背景

规格「工具契约」要求 `read_file`、`write_file`、`edit_file`、`bash` 自动执行，路径限定在 session 所属 workspace 根内，错误以结构化工具结果回传模型。原实现只有 `ToolBox` 窄 trait 与 `NoTools` 占位执行器：每次工具调用都返回 `unknown_tool`，模型拿不到工具定义，也无法读写文件。

## 决策

**工具集合由 `ToolRegistry` 实现 `ToolBox`**。`ToolBox` 增加 `definitions()`，返回四个工具的名称、说明与 JSON Schema 参数。引擎在每轮请求前取出定义传给 `ModelProvider::stream_chat`，因此请求体携带全部工具声明。`NoTools` 保留为空注册表，只用于不涉及工具的测试。

**路径必须是 workspace 根内的相对路径**。解析分三步：拒绝空路径与绝对路径；拒绝任何 `..`、根与前缀组件；把相对路径拼到根上再做 containment 检查。

- 读取与编辑：对完整路径 `canonicalize`，因此沿途符号链接被解析；规范化结果必须仍以根为前缀，否则 `path_escape`。目标不存在是 `file_not_found`。
- 写入：对父目录 `canonicalize`，父目录必须存在（`parent_not_found`）且位于根内；目标本身可以不存在。目标已存在时校验其规范化结果，因此指向根外的符号链接被拒绝；悬空符号链接同样拒绝。
- `bash` 的 `cwd` 用与读取相同的解析，默认根。

**每个工具的错误码是稳定枚举**。`read_file` 区分 `not_utf8`、`read_limit_exceeded`、`invalid_line_range`；`write_file` 用 `parent_not_found`；`edit_file` 区分 `edit_not_found` 与 `edit_not_unique`；`bash` 用 `bash_timeout`。新增两个类别：`io_error` 表示不属于上述类别的操作系统失败（例如路径是目录、无法写入），`cancelled` 表示工具被取消信号中止。参数不是合法 JSON、含未知字段或缺少必填字段一律 `invalid_arguments`。

**工具语义**：

- `read_file`：只读 UTF-8；默认 256 KiB 字节上限，超限报错。行区间为 1 起、含两端；起始行为 0、结束行早于起始行、区间超出文件行数都是 `invalid_line_range`，不做静默截断。输出每行为 `行号<TAB>内容`，各带换行。
- `write_file`：覆盖写入，不隐式创建目录，返回写入字节数。
- `edit_file`：`old_string` 必须恰好匹配一次；空 `old_string` 是 `invalid_arguments`。替换后写回，返回替换次数与字节数。
- `bash`：以 `sh -c` 执行，stdin 关闭，cwd 默认根。stdout 与 stderr 收集到同一结果块（stdout 在前）。输出默认上限 64 KiB，超出时保留前缀并在结果中标注截断；截断不是错误。返回退出码与耗时。非零退出不是工具错误：错误码表没有对应类别，退出码本身即结果。命令输出按 lossy UTF-8 解码，因为命令输出是字节流而不是文档。

**超时与取消杀掉整个进程组**。子进程以自身 pid 作为进程组 id 启动，超时（默认 120 秒）或取消时向 `-pid` 发送 `SIGKILL`。进程组由一个 `Drop` 守卫持有，因此引擎在取消时丢弃工具 future 也会清理进程组，而不只杀掉直接子进程。

**文件操作为同步调用**。读取规模受字节上限约束，写入内容是模型已生成的字符串，因此不引入额外的任务与通道。

## 后果

- 工具结果与调用一起持久化在 assistant 节点的 `tool_calls` 中，重启与回放读到相同内容。
- 工具失败不终止 run：结果回传模型，循环继续；只有模型错误、上下文超限、步数超限与中断才结束 run。
- 路径解析与写入之间存在时间窗口（TOCTOU）。安全边界是本机单用户与 workspace 根，不做容器沙箱、命令审查与网络限制。
- 测试用真实临时目录与真实子进程验证 containment、超时与取消；`bash` 超时以 300 ms 配置覆盖，不依赖默认 120 秒。
