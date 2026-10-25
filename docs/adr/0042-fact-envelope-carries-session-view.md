# ADR 0042: 事实信封携带 session 视图

- 状态：已接受。取代 ADR 0013 中「`queue_item_removed` 由客户端递减队列长度」的约定；细化 ADR 0012 的单一写入线
- 日期：2026-10-25

## 背景

事实事件原先在载荷里携带完整的 `SessionMetadata`：10 个变体带 `session` 字段，2 个变体只带 `session_id`，2 个 workspace 变体不带 session 引用。这带来三个问题。

第一，载荷混了两类信息。`busy`、队列长度与 `updated_at` 不是事实本体，而是事实提交之后的派生视图；它们由每条写命令各自手写或从快照抄回。写错就是事实写错，而客户端直接用 `busy` 推导侧栏状态、发送模式与文档标题。

第二，形状不一致导致客户端补偿。`queue_item_removed` 只带 `session_id`，客户端只好用 `queueLength - 1` 自行修正队列长度。

第三，同一事实在不同命令里可能带不同的视图，因为每条命令各自计算。

## 决策

**事实的身份留在载荷，派生视图放进信封。** `FactEvent` 的 10 个变体把 `session: SessionMetadata` 换成 `session_id: SessionId`，于是 12 个 session 变体形状相同，2 个 workspace 变体返回 `None`。`CommittedEvent`、`StreamEvent::Fact` 与 `HubStreamEvent::Fact` 各增加 `session: Option<SessionMetadata>`，含义是「该事实提交后的 session 元数据」。

**入口是唯一的视图推导点。** `Store::command`、`command_idle` 与 `command_root` 在闭包返回后推导视图：队列长度取快照计数加本次入队数减本次出队数；`busy` 取本次最后一条 run 变更的状态，没有 run 变更时取快照值；session 记录与快照记录不同则把 `updated_at` 设为本次时间戳并登记 `update_session`，相同则不写 session 行。视图落在本次提交的每条事件行上。

**`session: None` 表示该事实没有 session 视图**：workspace 事件、`session_deleted`，以及迁移之前记录的 `queue_item_removed`。最后一类的视图当时没有落库，无法回填；客户端在这一类上只删队列项，不改 session 元数据。

**`event` 表新增 `session` 列。** 迁移 0009 回填旧行：把旧载荷里的 `session` 对象搬到新列，把载荷里的 `session` 键换成 `sessionId`（取旧对象里的 `id`），并从载荷中摘除。回填之后旧事件与新事件形状一致。

## 后果

- **破坏性协议变更。** SSE 事实事件的载荷不再有 `session` 字段，信封新增 `session`。客户端必须同时读两处：身份看载荷，视图看信封。
- 客户端不再自行补偿队列长度：`queue_item_removed` 现在也带视图，自减补丁删除。
- 写命令不再手写 session 的队列长度、`busy` 与 `updated_at`，也不登记 `update_session` 或取当前时间；这些只在命令入口推导一次。命令读快照的队列长度与 `busy` 用于判定，不改写视图。
- 记录未改动的命令不再推进 `updated_at`。例如入队一条消息（标题不变）与结束 run 都不再更新 session 行；视图里的 `updated_at` 因此是上一次记录变化的时间。
- 一次命令的提交仍是单条事务，事件与记录同时可见；信封的视图与提交后快照逐字段相等，这条不变量由 `crates/core/tests/command.rs` 覆盖。
- session 视图是六个字段的元数据，不是消息正文；ADR 0038 的「事件只带骨架，正文按需拉取」不受影响。
