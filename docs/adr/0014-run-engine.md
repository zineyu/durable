# ADR 0014: run 引擎与事件流

- 状态：已接受。并发槽位与 `DURABLE_MAX_CONCURRENCY` 部分由 ADR 0023 取代
- 日期：2026-10-25

## 背景

规格要求：用户消息入队后由 run 引擎取出执行；assistant 文本流式显示；工具调用循环；全局并发槽位；取消；进程重启后把 running 的 run 标记为 failed。原实现只有入队，没有 run、模型调用与事件流。

## 决策

**run 是持久实体**。`run` 表保存 `id`、`session_id`、`status`、`failure`、`created_at`、`updated_at`。`status` 取 `running`、`succeeded`、`failed`、`cancelled`；`queued` 由队列项承载，不写入 run 表。`message` 增加 `run_id`，因此一次 run 的用户消息与 assistant 消息都可追溯到 run。run id 由 `meta.next_run_id` 全局分配，分配先于提交，失败留空洞、永不复用。

**一次 run 产生一个 assistant 节点**。run 开始时在一次 `Changeset` 内完成：删除队首队列项、写入 run 行、写入用户节点与 assistant 节点、把 active leaf 移到 assistant 节点、追加 `message_appended`（两次）、`queue_item_removed` 与 `run_started`。assistant 节点以空内容开始，因为只请求工具的 assistant 消息可以没有文本。

**部分输出节流落库，delta 只走 SSE**。assistant 文本每 100ms 写入一次节点，写入不产生事实事件。run 结束时再把终文本与工具调用记录写一次，并追加 `message_updated` 与 `run_finished`。刷新页面时 `bootstrap` 与消息树读到已落库的部分输出；未落库的增量只在事件流中。

**工具调用与结果挂在 assistant 节点上**。`tool_calls` 保存 `id`、`name`、`arguments` 与结构化 `ToolOutcome`（`succeeded { content }` 或 `failed { code, message }`）。请求下一轮时，引擎把该节点展开为 OpenAI 的 assistant 消息加若干 `tool` 消息。工具失败不是 run 失败：结果回传模型，循环继续。

**失败类别是稳定枚举**：`model_upstream`、`context_limit_exceeded`、`step_limit_exceeded`、`interrupted`。上下文按字符数限制（默认 200000），步数默认 50，全部可经环境变量配置：`DURABLE_MAX_CONCURRENCY`（默认 4）、`DURABLE_CONTEXT_CHAR_LIMIT`、`DURABLE_MAX_STEPS`。

**Store 持有事件总线**。`Store::commit` 在存储事务提交成功后把 `CommittedEvent` 发布为 `StreamEvent::Fact`，因此「先提交、后发布」由内核保证，所有调用方一致。`StreamEvent::Delta` 由引擎发布，不落库。事件信封带全局事件 id、session id 与时间戳；delta 不带事件 id，因此不会推进客户端的续传游标。

**单一 SSE 端点**。`GET /api/events?after=<id>` 先订阅总线，再按 id 升序回放 `after` 之后的事实事件，然后接入实时事件；实时事件中 id 不大于回放最大 id 的事实事件被跳过。事实事件写 SSE `id:` 行，delta 不写。每 15 秒发送保活注释。

**调度按 session 串行、跨 session 并发**。每个有待执行项的 session 有一个驱动任务；驱动先取得全局信号量槽位，再调用 `Store::start_run`（在写入线内读队首并提交），队列为空时释放槽位并退出。`tokio::Semaphore` 的 FIFO 等待队列提供 session 间的公平轮转。驱动集合用 `std::sync::Mutex`，避免驱动任务在递归 spawn 中产生非 `Send` future。

**取消是显式信号**。`Cancellation`（原子标志加 `Notify`）经 `select!` 中止模型请求与工具执行；引擎随后以 `cancelled` 结束 run，保留已收到的部分文本。删除 session 先取消并等待 run 结束，再删除记录，避免终态写入命中已删除的行。

**启动恢复**。`AgentState::start` 先把仍为 `running` 的 run 改为 `failed`（`interrupted`），再为仍有队列项的 session 启动驱动，因此重启后队列继续执行。

## 后果

- 消息树、run 状态、队列与事件在一次提交内同时可见；客户端可只靠事实事件重建，也可用消息树校正。
- 运行中的 session 在 `bootstrap` 与 `session_tree` 中 `busy` 为真；`busy` 由 `run` 表中 running 计数派生，不再写死。
- 工具执行器通过窄 `ToolBox` trait 注入；未注册的工具返回结构化失败，run 继续。
- `ErrorCode` 增加 `queue_item_not_found`；run 失败类别使用独立的 `RunFailureCode`，因为它是 run 终态而非命令错误。
