# 术语表

| 术语 | 定义 |
| --- | --- |
| workspace | 注册到 agent 的目录。身份为规范化绝对路径。文件与 bash 操作的安全边界。 |
| workspace root containment | 路径规范化后必须仍位于 workspace 根内。绝对路径、`..` 逃逸、符号链接逃逸均拒绝。 |
| agent binary | Rust 进程。提供 HTTP API，执行 agent 循环、工具调用，持有 SQLite。 |
| ui binary | Rust 进程。内嵌并托管 React 静态资源，把 `/api/*` 反代到 agent。 |
| session | 绑定唯一 workspace 的对话单元。含消息树、队列、active leaf。 |
| run | 一次用户消息触发的执行。从发起模型请求开始，到 assistant 不再请求工具结束。 |
| step | run 内的一次模型请求及其工具调用。 |
| message tree | session 内消息节点构成的树。节点含 id、parent_id、role、内容、run 元数据。 |
| active leaf | session 当前选中的叶子节点。 |
| transcript | root 到 active leaf 的路径。即发给模型的上下文。 |
| branch | 共享同一父节点的兄弟消息构成的替代历史。 |
| fact event | 持久化到 SQLite 的领域事件。token 增量不属于 fact event。 |
| durable 内核 | `crates/core`（包名 `durable`）中的领域与持久化层。持有单一写入线、领域类型与存储。 |
| 写入线 | 串行化写入的互斥边界。读-改-写与一次原子提交都在同一条写入线内完成。 |
| changeset | 一次提交登记的变更集合，顺序即意图，在单条存储事务内整体生效。 |
| delta | 流式 token 增量。仅经 SSE 实时传输，不落库。 |
| queue item | 等待执行的用户消息。session 级 FIFO，持久化。 |
| bootstrap | 客户端启动时获取的元数据快照与事件游标。 |
| scripted provider | 测试用本地 OpenAI 兼容 SSE 服务。按脚本返回响应，不替换被测代码。 |
| seam | 规格定义的公开测试边界。 |
