# 术语表

| 术语 | 定义 |
| --- | --- |
| workspace | 绑定唯一 host 的目录。身份为 host 名加规范化绝对路径。文件与 bash 操作的安全边界。 |
| workspace root containment | 路径规范化后必须仍位于 workspace 根内。绝对路径、`..` 逃逸、符号链接逃逸均拒绝。 |
| 工具定义 | 每轮模型请求携带的 function 声明：名称、说明与 JSON Schema 参数。 |
| 工具失败 | 结构化的 `ToolOutcome::Failed`，含稳定错误码与说明。工具失败回传模型，run 继续。 |
| 读取上限 | `read_file` 的字节上限，默认 256 KiB。超出是错误，不是截断。 |
| 输出截断 | `bash` 输出超过上限（默认 64 KiB）时保留前段并在结果中标注。截断不是错误。 |
| 进程组 | `bash` 子进程以自身 pid 作为进程组 id。超时或取消时杀掉整个进程组。 |
| agent | Rust 进程。唯一权威：模型调用、run 执行、消息树、队列、事件与浏览器入口，持有 SQLite。原 `hub`。不写作 hub。 |
| host | 一台提供 workspace 与工具执行的机器。身份为名称，连接参数是一个 SSH 目标字符串。不写作 node。 |
| host 表 | agent 的 SQLite 中登记的 host 名单。host 名即身份。 |
| host 启用 | `enabled`。host 表上的开关。停用时 agent 不建立 link，指向该 host 的命令返回 host_unavailable。 |
| host 更新 | host_updated。SSH 目标或启用标志变化时写入的事实事件。 |
| 手动重连 | host 表命令：丢弃当前 link 并按退避起点立即重建，不改变 host 状态以外的内容。 |
| host 状态 | `ok`、`unreachable`、`incompatible`。状态不为 `ok` 时指向该 host 的命令全部禁用。 |
| 状态说明 | 与 host 状态同行的人读说明：该 host 为什么不是 `ok`。取失败原文；`ok` 时为空。 |
| 未知 host | 请求指向的 host 名不在 host 表中。host 改名后的旧链接落在此状态。 |
| executor | host 上的守护进程。执行一次工具调用，并裁决文件系统事实：workspace 路径校验与四个工具。不持有数据库。不写作 proxy。 |
| link | agent 与 executor 之间的帧通道。传输有两种：本地子进程与 SSH 子进程。不写作 tunnel。 |
| 帧 | link 上的一个长度前缀 JSON 消息。只有四种：请求、结果、取消、心跳。 |
| 本地传输 | 本机 host 的传输：agent 直接启动 executor 子进程，标准输入输出即 link。 |
| SSH 传输 | 远端 host 的传输：agent 启动 ssh 子进程并在其中运行 executor，标准输入输出即 link。 |
| 安装 | 首次连接远端 host 时，把与 agent 同版本的 executor 二进制推送到该 host 的标准数据目录，并校验构建版本。 |
| link 延迟 | `latency_ms`。最近一次心跳往返的毫秒数。进程内计数。 |
| session | 绑定唯一 workspace 的对话单元。含消息树、队列、active leaf。session 因而属于该 workspace 的 host。 |
| run | 一次用户消息触发的执行。从发起模型请求开始，到 assistant 不再请求工具结束。 |
| run 状态 | `running`、`succeeded`、`failed`、`cancelled`。排队中的 run 由队列项表示。 |
| run 失败类别 | `model_upstream`、`model_rate_limited`、`context_limit_exceeded`、`step_limit_exceeded`、`storage_failed`、host_unreachable、`interrupted`（只为读出历史值，不再产生）。 |
| step | run 内的一次模型请求及其工具调用。 |
| message tree | session 内消息节点构成的树。节点含 id、parent_id、role、内容块与 run 元数据。一个节点是一条消息。 |
| 内容块 | 一条消息正文中的一个类型化片段：文本、推理、工具调用、工具结果、图像。块序即产生顺序。 |
| 工具结果节点 | 承载一条工具结果的独立节点，`role` 为 tool，父节点为发起该调用的 assistant 节点。 |
| 压缩消息 | 链上的特殊消息，正文是压缩产出的摘要。装配上下文时从 active leaf 向上回溯到此消息即停止，它替代它之前的全部历史。 |
| 已中断消息 | 流式生成崩溃或用户取消时入树的 assistant 消息，正文是当时已经产生的部分输出。骨架上的 `interrupted` 标记区分它与完整回答；续跑从它之后继续，装配上下文时它的正文与续跑产出的文本合并成一条。 |
| active leaf | session 当前选中的叶子节点。 |
| transcript | 从 active leaf 向上回溯到根或最近一条压缩消息的路径，压缩消息在前。即发给模型的上下文。 |
| branch | 共享同一父节点的兄弟消息构成的替代历史。 |
| 队列锚点 | 队列项要接续的用户节点。空表示 run 开始时在当前 active leaf 下新建用户节点。 |
| 编辑用户消息 | 在目标用户消息的父节点下创建兄弟用户节点，把 active leaf 移到新节点并入队执行。旧子树保留。 |
| 重试 assistant | 以目标 assistant 的父用户节点为锚新建 run，产生同父的 assistant 兄弟节点，不重复用户消息。 |
| 切换 active leaf | 把 active leaf 移到目标子树中最近创建的叶子。只修改 active leaf，并写入 `active_leaf_changed` 事实事件。 |
| fact event | 持久化到 SQLite 的领域事件。token 增量不属于 fact event。 |
| 工具调用事实 | 一次工具执行作为独立 tool 节点入树：执行前以执行中状态写入，执行完写入结局。 |
| 工具幂等 | 工具定义上的标记：重复执行给出相同结果且无额外副作用。只有幂等工具允许在崩溃后重跑。 |
| 结果未知 | 非幂等工具在执行中崩溃或 link 断开后写下的工具结局。既不是成功也不是工具失败，表示我们丢失了对结果的观测。 |
| durable 内核 | `crates/core`（包名 `durable`）中的领域与持久化层。持有单一写入线、领域类型与存储。 |
| 写入线 | 串行化写入的互斥边界。读-改-写与一次原子提交都在同一条写入线内完成。 |
| changeset | 一次提交登记的变更集合，顺序即意图，在单条存储事务内整体生效。 |
| 写命令 | 一次写入的完整意图：前置判定、变更登记与返回值。不含加锁、视图推导与发布。 |
| 命令入口 | `Store::command`、`Store::command_idle` 与 `Store::command_root`。承载加锁、轻量快照、session 视图推导、提交与发布。 |
| 根命令 | 不读 session 快照的写命令：workspace、host 与新建 session。入口是 `Store::command_root`。 |
| 轻量快照 | 命令入口读取的 session 状态：记录、队列长度与是否有 running 的 run。不加载消息树。 |
| session 视图 | 事实信封上的 `session` 字段，即该事实提交后的 session 元数据。为空表示该事实不属于任何 session，或该事实删除了 session。 |
| delta | 流式 token 增量。仅经 SSE 实时传输，不落库。 |
| 限流 | 模型供应商对请求速率的拒绝，通常表现为 HTTP `429` 或带 `Retry-After` 的响应。 |
| 重试提示 | `retry` 事件：限流等待期间广播的重试序号、上限与等待时长。仅实时，不落库，不推进游标。 |
| 退避 | 限流重试的等待策略：底数逐次翻倍，单次不超过上限；`Retry-After` 存在时以它为准。 |
| queue item | 等待执行的用户消息。session 级 FIFO，持久化。 |
| 插话 | `mode=steer` 提交的用户消息。它在提交时就是用户节点，在步骤边界接入当前 run。 |
| 待插入 | 插话的 `pending_steer` 状态：节点已入树，尚未接入链尾。run 结束前未接入即降级为排队。 |
| 步骤边界 | 引擎发模型请求与收尾之前的位置。插话只在此处接入；工具执行期间不接入。 |
| 入队 | 把用户消息写入 session 的 FIFO 队列。此阶段不创建消息节点，不移动 active leaf。 |
| 自动标题 | 首条用户消息的首行前 50 个字符决定的 session 标题。重命名后不再自动更新。 |
| 并发 | session 内的 run 严格串行；session 之间没有并发上限。 |
| 部分输出 | run 进行中已收到但尚未定型的 assistant 内容。只在内存与实时增量里，不落库。 |
| 消息定型 | assistant 消息在流正常结束时一次入树。定型前的部分输出不属于历史。 |
| 续跑 | 进程启动时，对状态仍是 running 的 run 从树尾推导断点并继续执行。 |
| 消息骨架 | 不含块正文的节点描述：id、父指针、角色、run、时间，以及各块的类型、字节数与状态。 |
| 正文拉取 | 客户端按消息 id 批量获取块正文的读路径。事件与快照只给骨架。 |
| 事件信封 | 事件流中的一条消息。含全局事件 id、session id、session 视图、时间戳与负载。delta 不带事件 id。 |
| 事件游标 | 客户端用于 `after` 重放的事件 id。首次装配时取快照水位；事实事件应用后前移；应用水位更老的快照时回退。 |
| 回放 | 按事件 id 升序重新发送游标之后的事实事件。 |
| 回放 | 按事件 id 升序重新发送游标之后的事实事件。 |
| 水位 | 快照读取事务开始时的全局最大事实事件 id。快照包含该 id 及之前的全部事实。快照可以额外包含更新的事实，客户端按 id 去重。 |
| resync | 重连刷新：重新获取 bootstrap 与当前 session tree，把游标回退到快照水位后重开事件流。由连接错误触发。 |
| 协议版本 | agent 与 executor 之间的帧协议版本。握手不符时该 host 为 `incompatible`。 |
| 取消信号 | 中止在途模型请求与工具执行的显式信号。跨 link 时以取消帧传递。取消后 run 以 `cancelled` 结束，未定型的内容丢弃。 |
| bootstrap | 客户端启动时获取的元数据快照与事件游标。 |
| 客户端状态 | 浏览器内由纯 reducer 从 bootstrap、事实事件、token 增量与 session 树快照归约出的状态：host 表、workspace、session 与当前 session 的树。呈现所需的数据不在此处，由派生视图给出。 |
| 派生视图 | 由客户端状态经纯函数推导出的呈现数据，代码在 `client/src/view/derived.ts`。session 摘要全算；完整视图只对可见 session 计算。组件只接收派生后的 props，不读状态。 |
| session 摘要 | 一个 session 的标题、状态、未读数、最近结局、队列长度与更新时间。侧栏分组、命令面板与运行中列表读它。 |
| session 派生视图 | 一个 session 的完整派生数据：摘要、transcript（消息连同正文、消息状态与说明、分支位置）、工具结局、队列、未定型占位与空闲判定。主面板、session 内搜索与 Markdown 导出读它。 |
| 命令面板 | `Ctrl/⌘+K` 打开的单一面板。分 session（跨 host）、动作与 host 三组，条目可模糊过滤并用键盘执行。 |
| 草稿 | 按 session 存在浏览器 `localStorage` 的未发送输入。含折叠的长粘贴块。 |
| 未读标记 | 客户端状态里按 `lastSeenAt` 推导的 session 状态：未读、最近完成或最近失败。只在本地，不入库。 |
| 正在运行面板 | 侧栏中汇总全部 host 运行中 run 的面板。代码标识符 `ActivityPanel`。 |
| 连接状态 | 客户端对 SSE 连接的可见状态：已连接、重连中、已断开。 |
| scripted provider | 测试用本地 OpenAI 兼容 SSE 服务。按脚本返回响应，不替换被测代码。 |
| 标准目录 | 应用按平台惯例使用的可写数据与日志目录。macOS 用 `Library` 子目录，类 Unix 平台用 XDG 基础目录，Windows 用 `LOCALAPPDATA`。 |
| 日志文件 | 标准日志目录中按天轮转的 JSON Lines 文件，文件名为 `<prefix>.<YYYY-MM-DD>.log`，保留最近 14 个。 |
| 日志过滤器 | `DURABLE_LOG` 环境变量的指令。语法同 `RUST_LOG`，默认 `info`。 |
| 默认数据库路径 | 未指定 `--database` 时使用的标准数据目录下的 `agent.sqlite`。 |
| seam | 规格定义的公开测试边界。 |
