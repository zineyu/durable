# ADR 0019: 工具调用以事实事件增量广播

- 状态：已接受
- 日期：2026-11-02

## 背景

规格把「工具调用、工具结果」列为持久化事实事件，user story 9 要求用户看到每个工具调用的卡片。原实现只在 run 结束时随 `message_updated` 携带 `tool_calls`：run 进行中（尤其是长时间的工具步）客户端看不到任何工具卡片，第二个标签页也收不到。

直接复用 `message_updated` 广播部分输出会把这些快照文本写进事件表，与「delta 只走 SSE、不落库」的约定冲突。

## 决策

**每次工具执行完成后追加一条 `tool_call_recorded` 事实事件**。负载含 `session` 元数据、`message`（assistant 节点 id）与 `tool_call`（`id`、`name`、`arguments`、结构化 `ToolOutcome`）；不含 assistant 文本。

`Store::record_tool_call` 在一次写入线内读回 assistant 节点、追加记录、写回 `tool_calls`，并在同一 `Changeset` 内登记事实事件。工具调用记录因此既在节点上，也在事件流与事件表中。

引擎在每个工具完成后立即调用它，`flush_message` 仍只负责节流写入文本。run 结束时 `message_updated` 携带完整记录覆盖节点，客户端按 `id` 去重，两种来源结果一致。

## 后果

- run 进行中客户端即可显示工具卡片；重连回放与 `session_tree` 快照给出同一份记录。
- 事件表只增 `tool_call_recorded`，不会写入 assistant 文本快照，delta 仍不落库。
- 客户端 reducer 的事件分支必须穷尽新事件：`tool_call_recorded` 追加到对应节点，重复到达时保持原状态。
