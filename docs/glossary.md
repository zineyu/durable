# 术语表

| 术语 | 定义 |
| --- | --- |
| workspace | 注册到 agent 的目录。身份为规范化绝对路径。文件与 bash 操作的安全边界。 |
| workspace root containment | 路径规范化后必须仍位于 workspace 根内。绝对路径、`..` 逃逸、符号链接逃逸均拒绝。 |
| 工具定义 | 每轮模型请求携带的 function 声明：名称、说明与 JSON Schema 参数。 |
| 工具失败 | 结构化的 `ToolOutcome::Failed`，含稳定错误码与说明。工具失败回传模型，run 继续。 |
| 读取上限 | `read_file` 的字节上限，默认 256 KiB。超出是错误，不是截断。 |
| 输出截断 | `bash` 输出超过上限（默认 64 KiB）时保留前段并在结果中标注。截断不是错误。 |
| 进程组 | `bash` 子进程以自身 pid 作为进程组 id。超时或取消时杀掉整个进程组。 |
| agent binary | Rust 进程。提供 HTTP API，执行 agent 循环、工具调用，持有 SQLite。 |
| hub | Rust 进程。唯一浏览器入口。内嵌并托管 React 静态资源，持有节点表，把全部 node 的协议合并成单一 `/api/*` 与单一事件流。原 `ui binary`。 |
| node | 一台机器上的一个 agent 进程与其 SQLite。node 只有 agent，没有 hub。 |
| 节点表 | hub 的 SQLite 中登记的 node 名单。节点名即身份。 |
| 节点状态 | `ok`、`unreachable`、`incompatible`。状态不为 `ok` 时该节点的命令全部禁用。 |
| 未知节点 | URL 或请求指向的节点名不在节点表中。节点改名后的旧链接落在此状态。 |
| 合并 bootstrap | hub 汇总节点表、节点状态与可达节点元数据的单次快照。节点级失败不使整体失败。 |
| 节点作用域请求 | 路径以 `/api/n/<node>/` 开头的请求。hub 剥掉前缀后原样转发给该 node。 |
| session | 绑定唯一 workspace 的对话单元。含消息树、队列、active leaf。session 属于唯一 node。 |
| session 全局键 | `(node, session_id)` 二元组。session id 只在 node 内唯一。URL 用 `#/n/<node>/s/<session_id>`。 |
| run | 一次用户消息触发的执行。从发起模型请求开始，到 assistant 不再请求工具结束。 |
| run 状态 | `running`、`succeeded`、`failed`、`cancelled`。排队中的 run 由队列项表示。 |
| run 失败类别 | `model_upstream`、`context_limit_exceeded`、`step_limit_exceeded`、`interrupted`。 |
| step | run 内的一次模型请求及其工具调用。 |
| message tree | session 内消息节点构成的树。节点含 id、parent_id、role、内容、run 元数据。 |
| active leaf | session 当前选中的叶子节点。 |
| transcript | root 到 active leaf 的路径。即发给模型的上下文。 |
| branch | 共享同一父节点的兄弟消息构成的替代历史。 |
| 队列锚点 | 队列项要接续的用户节点。空表示 run 开始时在当前 active leaf 下新建用户节点。 |
| 编辑用户消息 | 在目标用户消息的父节点下创建兄弟用户节点，把 active leaf 移到新节点并入队执行。旧子树保留。 |
| 重试 assistant | 以目标 assistant 的父用户节点为锚新建 run，产生同父的 assistant 兄弟节点，不重复用户消息。 |
| 切换 active leaf | 把 active leaf 移到目标子树中最近创建的叶子。只修改 active leaf，并写入 `active_leaf_changed` 事实事件。 |
| fact event | 持久化到 SQLite 的领域事件。token 增量不属于 fact event。 |
| 工具调用事实 | 每次工具执行完成后追加到 assistant 节点并以 `tool_call_recorded` 广播的记录。不含 assistant 文本。 |
| durable 内核 | `crates/core`（包名 `durable`）中的领域与持久化层。持有单一写入线、领域类型与存储。 |
| 写入线 | 串行化写入的互斥边界。读-改-写与一次原子提交都在同一条写入线内完成。 |
| changeset | 一次提交登记的变更集合，顺序即意图，在单条存储事务内整体生效。 |
| delta | 流式 token 增量。仅经 SSE 实时传输，不落库。 |
| queue item | 等待执行的用户消息。session 级 FIFO，持久化。 |
| 入队 | 把用户消息写入 session 的 FIFO 队列。此阶段不创建消息节点，不移动 active leaf。 |
| 自动标题 | 首条用户消息的首行前 50 个字符决定的 session 标题。重命名后不再自动更新。 |
| 并发 | session 内的 run 严格串行；session 之间没有并发上限。 |
| 部分输出 | run 进行中已收到的 assistant 文本。节流落库，未落库部分只经事件流传输。 |
| 事件信封 | 事件流中的一条消息。含全局事件 id、session id、时间戳与负载。delta 不带事件 id。 |
| 事件游标 | 客户端用于 `after` 重放的事件 id。首次装配时取快照水位；事实事件应用后前移；应用水位更老的快照时回退。 |
| 回放 | 按事件 id 升序重新发送游标之后的事实事件。 |
| 水位 | 快照读取事务开始时的全局最大事实事件 id。快照包含该 id 及之前的全部事实。 |
| resync | 重连刷新：重新获取 bootstrap 与当前 session tree，把游标回退到快照水位后重开事件流。由广播落后或连接错误触发。 |
| 总线 | hub 为每个 node 维持常驻上游事件流并广播给全部客户端的角色。 |
| 广播序号 | hub 赋给每条广播事实信封的自增序号。客户端的续传游标。 |
| 回放窗口 | hub 内存中按广播序号保留的最近事实事件。游标落在窗口外即要求 resync。 |
| 协议版本 | `/api/health` 返回的协议版本。与 hub 期望不符时该节点为 `incompatible`。 |
| 取消信号 | 中止在途模型请求与工具执行的显式信号。取消后 run 以 `cancelled` 结束并保留部分输出。 |
| bootstrap | 客户端启动时获取的元数据快照与事件游标。 |
| 客户端状态 | 浏览器内由纯 reducer 从 bootstrap、事实事件、token 增量与 session 树快照推导的视图模型。 |
| 连接状态 | 客户端对 SSE 连接的可见状态：已连接、重连中、已断开。 |
| scripted provider | 测试用本地 OpenAI 兼容 SSE 服务。按脚本返回响应，不替换被测代码。 |
| 标准目录 | 应用按平台惯例使用的可写数据与日志目录。macOS 用 `Library` 子目录，类 Unix 平台用 XDG 基础目录，Windows 用 `LOCALAPPDATA`。 |
| 日志文件 | 标准日志目录中按天轮转的 JSON Lines 文件，文件名为 `<prefix>.<YYYY-MM-DD>.log`，保留最近 14 个。 |
| 日志过滤器 | `DURABLE_LOG` 环境变量的指令。语法同 `RUST_LOG`，默认 `info`。 |
| 默认数据库路径 | 未指定 `--database` 时使用的标准数据目录下的 `agent.sqlite` 或 `hub.sqlite`。 |
| seam | 规格定义的公开测试边界。 |
