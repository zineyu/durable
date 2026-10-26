# ADR 0017: 客户端状态推导与事件流接线

- 状态：已接受
- 日期：2026-10-19

## 背景

ADR 0002 定义单一 SSE 事件流与 `after` 游标，ADR 0004 与 ADR 0016 定义消息树、分支与 active leaf 的 server 语义。客户端此前只消费静态 fixture。规格要求客户端状态由纯 reducer 从事件流与消息树推导，组件不持有业务状态、不拼接对话历史；刷新、断线、重启后视图仍需与 server 一致。

## 决策

**单一纯 reducer 持有全部业务状态**。ADR 0044 在 reducer 之上加了派生视图层：本 ADR 的「组件只接收派生后的 props」由 `client/src/view/derived.ts` 兑现，reducer 的语义不变。`ClientState` 含模型名、workspace 与 session 元数据、按 session id 索引的树状态（节点、active leaf、队列、run）、连接状态、最后应用的事件 id 与当前错误。bootstrap、事实事件、token 增量、`session_tree` 快照、连接状态与错误都经同一个 `reduce(state, action)` 归约。组件只接收派生后的 props。

**transcript 由节点与 active leaf 推导**。transcript 是从 root 沿 `parent_id` 到 active leaf 的路径。分支切换器显示在存在兄弟（同一 `parent_id`）的节点上，序号取兄弟按 id 升序的位置。切换分支只调用 `POST /api/sessions/{id}/active-leaf`；新叶子仍由 server 按「目标子树中 id 最大的叶子」推导，客户端不复制该规则。

**事实事件按 id 单调去重**。事件 id ≤ 最后应用 id 时该事件被丢弃。因此重复事件与乱序到达的旧事件都不改变已有状态；每条事实事件都携带完整的 session 元数据快照，不需要合并增量字段。token 增量不携带事件 id，按到达顺序追加到目标 assistant 节点；run 结束的 `message_updated` 用完整文本覆盖增量结果。

**树状态惰性创建，快照权威**。消息、队列与 run 事实事件在对应 session 的树尚未加载时创建空的树状态并应用，使「bootstrap + `after` 回放」与「tree 快照」得到相同的 transcript。用户选中 session 时用 `GET /api/sessions/{id}/tree` 的快照整体替换该 session 的树状态。

**断线由客户端显式续传**。事件流模块记录最后应用的事实事件 id，断线后按该 id 重开 `GET /api/events?after=`；`EventSource` 自动重连会复用旧 URL 与 `Last-Event-ID`，server 只认 `after` 查询参数，因此由客户端主动重建连接。连接状态与错误横幅经 reducer 驱动。

## 后果

- seam 2 的 reducer 测试直接覆盖增量合并、去重、乱序、分支路径与多 session 隔离；真实 SSE 传输与重连计时不在测试 seam 内。
- server 是分支叶子与队列执行顺序的唯一权威；客户端不实现第二套推导。
- 编辑与重试在 session 忙或队列非空时置灰；不满足前端置灰的请求仍返回显式冲突错误并显示在错误横幅。
