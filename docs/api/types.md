# 协议类型清单

由 `devenv shell -- durable-typegen` 从 Rust 类型生成，与 `client/src/api/generated/` 同一来源。不要手改。

## BootstrapResponse

```ts
export type BootstrapResponse = { model: string, maxEventId: number, workspaces: Array<WorkspaceMetadata>, sessions: Array<SessionMetadata>, };
```

## ContentKind

```ts
/**
 * 内容块的类型标签，与 [`ContentPart`] 的变体一一对应。
 */
export type ContentKind = "text" | "reasoning" | "tool_call" | "tool_result" | "image";
```

## ContentPart

```ts
/**
 * 一条消息正文中的一个类型化片段。块序即产生顺序。
 */
export type ContentPart = { "kind": "text", text: string, providerMeta: ProviderMeta | null, } | { "kind": "reasoning", text: string, providerMeta: ProviderMeta | null, } | { "kind": "tool_call", callId: string, name: string, arguments: string, providerMeta: ProviderMeta | null, } | { "kind": "tool_result", callId: string, state: ToolResultState, providerMeta: ProviderMeta | null, } | { "kind": "image", mediaType: string, data: string, providerMeta: ProviderMeta | null, };
```

## CreateSessionRequest

```ts
export type CreateSessionRequest = { workspace: string, title: string | null, };
```

## DeltaKind

```ts
/**
 * 进行中的增量属于哪种块。
 */
export type DeltaKind = "text" | "reasoning";
```

## EditMessageRequest

```ts
export type EditMessageRequest = { text: string, };
```

## ErrorCode

```ts
export type ErrorCode = "workspace_path_invalid" | "workspace_duplicate" | "workspace_not_found" | "workspace_not_empty" | "session_not_found" | "queue_item_not_found" | "message_not_found" | "message_role_mismatch" | "run_in_progress" | "queue_not_empty" | "node_name_taken" | "node_not_found" | "invalid_title" | "invalid_message" | "invalid_request" | "internal";
```

## ErrorResponse

```ts
export type ErrorResponse = { code: ErrorCode, message: string, };
```

## FactEvent

```ts
/**
 * 持久化到 SQLite 的领域事实事件。
 *
 * 载荷只带事实身份与事实本体；事实提交后的 session 视图在事件信封上（见
 * [`CommittedEvent`]），不在载荷里。同一变体因此不因元数据变化而改变。
 */
export type FactEvent = { "kind": "workspace_registered", workspace: WorkspaceMetadata, } | { "kind": "workspace_removed", path: WorkspacePath, } | { "kind": "session_created", sessionId: SessionId, } | { "kind": "session_renamed", sessionId: SessionId, } | { "kind": "session_deleted", sessionId: SessionId, } | { "kind": "message_appended", sessionId: SessionId, message: MessageSkeleton, } | { "kind": "message_updated", sessionId: SessionId, message: MessageSkeleton, } | { "kind": "active_leaf_changed", sessionId: SessionId, activeLeaf: number | null, } | { "kind": "message_queued", sessionId: SessionId, item: QueueItem, } | { "kind": "queue_item_removed", sessionId: SessionId, itemId: number, } | { "kind": "steer_applied", sessionId: SessionId, runId: number, itemId: number, messageId: number, } | { "kind": "steer_downgraded", sessionId: SessionId, item: QueueItem, } | { "kind": "run_started", sessionId: SessionId, run: RunRecord, } | { "kind": "run_finished", sessionId: SessionId, run: RunRecord, };
```

## HealthResponse

```ts
export type HealthResponse = { status: HealthStatus, version: string, protocolVersion: number, };
```

## HealthStatus

```ts
export type HealthStatus = "ok";
```

## HubBootstrap

```ts
/**
 * 合并 bootstrap。节点级失败体现在各自的 `status` 与 `detail` 里。
 */
export type HubBootstrap = { nodes: Array<NodeSnapshot>, 
/**
 * 当前最大广播序号。客户端用它作为续传游标的起点。
 */
maxHubSeq: number, };
```

## HubErrorCode

```ts
export type HubErrorCode = "node_name_invalid" | "node_name_taken" | "node_not_found" | "node_unavailable" | "invalid_request" | "internal";
```

## HubErrorResponse

```ts
export type HubErrorResponse = { code: HubErrorCode, message: string, };
```

## HubHealth

```ts
export type HubHealth = { status: HubStatus, version: string, protocolVersion: number, };
```

## HubStatus

```ts
export type HubStatus = "ok";
```

## HubStreamEvent

```ts
/**
 * 广播流中的一条事件。带 `hub_seq` 的进入回放窗口；增量与 resync 不带。
 */
export type HubStreamEvent = { "type": "fact", hubSeq: number, node: NodeName, id: number, sessionId: SessionId | null, 
/**
 * 事实提交后的 session 视图；不属于任何 session 或删除 session 时为空。
 */
session: SessionMetadata | null, timestamp: number, event: FactEvent, } | { "type": "delta", node: NodeName, sessionId: SessionId, messageId: number, kind: DeltaKind, text: string, timestamp: number, } | { "type": "retry", node: NodeName, sessionId: SessionId, 
/**
 * 即将进行的重试序号，从 1 开始。
 */
attempt: number, 
/**
 * 重试次数上限。
 */
maxAttempts: number, 
/**
 * 本次等待时长。
 */
delayMs: number, } | { "type": "node_registered", hubSeq: number, node: NodeMetadata, } | { "type": "node_removed", hubSeq: number, name: NodeName, } | { "type": "node_updated", hubSeq: number, node: NodeMetadata, } | { "type": "node_status_changed", hubSeq: number, name: NodeName, status: NodeStatus, detail: string | null, } | { "type": "resync", reason: ResyncReason, };
```

## MessageBodiesRequest

```ts
export type MessageBodiesRequest = { messageIds: number[], };
```

## MessageBodiesResponse

```ts
export type MessageBodiesResponse = { messages: Array<MessageBody>, };
```

## MessageBody

```ts
/**
 * 按消息 id 批量拉取的块正文。
 */
export type MessageBody = { id: number, parts: Array<ContentPart>, };
```

## MessageRole

```ts
/**
 * 消息在树中的角色。树里没有 `system`：系统提示由请求装配层提供。
 */
export type MessageRole = "user" | "assistant" | "tool" | "compaction";
```

## MessageSkeleton

```ts
/**
 * 消息骨架：事件与快照携带的结构描述，不含块正文。
 */
export type MessageSkeleton = { id: number, parentId: number | null, role: MessageRole, 
/**
 * 该消息是流式中断后入树的部分输出。迁移 0010 之前的事件行与载荷没有这个字段。
 */
interrupted: boolean, runId: number | null, createdAt: number, parts: Array<PartSkeleton>, };
```

## MessageText

```ts
/**
 * 用户输入正文。保留原始内容，只要求去掉空白后非空。
 */
export type MessageText = string;
```

## NodeMetadata

```ts
/**
 * 节点表中的一行。`url` 是 hub 连接该节点 `agent` 的基地址。
 */
export type NodeMetadata = { name: NodeName, url: string, 
/**
 * 停用的节点不建上游订阅，命令一律返回不可用。
 */
enabled: boolean, createdAt: number, updatedAt: number, };
```

## NodeName

```ts
/**
 * 节点名。节点名即身份，也是 URL 路径段，因此限制为路径安全字符。
 */
export type NodeName = string;
```

## NodeSnapshot

```ts
/**
 * 侧栏与 bootstrap 看到的单个节点。`bootstrap` 只有 `ok` 的节点才有。
 */
export type NodeSnapshot = { name: NodeName, url: string, 
/**
 * 停用的节点不建上游订阅，命令一律返回不可用。
 */
enabled: boolean, status: NodeStatus, detail: string | null, 
/**
 * 最后一条事实事件自带的时间戳。进程内计数，不落库。
 */
lastEventAt: number | null, 
/**
 * 最近一次 health 的往返延迟。进程内计数，不落库。
 */
latencyMs: number | null, bootstrap: BootstrapResponse | null, };
```

## NodeStatus

```ts
/**
 * 节点状态。只有 `ok` 的节点接受命令。
 */
export type NodeStatus = "ok" | "unreachable" | "incompatible";
```

## PartSkeleton

```ts
/**
 * 块骨架：序号、类型、字节数与状态。
 */
export type PartSkeleton = { seq: number, kind: ContentKind, size: number, state: PartState, };
```

## PartState

```ts
/**
 * 块在骨架中的状态。只有工具结果块会出现非 `complete` 的取值。
 */
export type PartState = "complete" | "running" | "unknown";
```

## ProviderMeta

```ts
/**
 * provider 私有的不透明元数据：签名、thought signature、图像令牌等。
 *
 * 领域不解释它的内容，也不允许单独写入；它随所属块一起存取。
 */
export type ProviderMeta = unknown;
```

## QueueItem

```ts
/**
 * 等待执行的用户消息。session 级 FIFO，按 id 升序执行。
 *
 * `anchor` 是本次执行接续的用户节点。`None` 表示 run 开始时在当前 active leaf 下新建用户节点；
 * `Some` 表示接续已存在的用户节点（编辑产生的新节点、重试 assistant 的父节点或待插入的插话）。
 *
 * `pending_steer` 表示该项是等待插入当前 run 的插话。插话在提交时已经是一条用户节点；
 * run 结束前未被消费时降级为普通排队，节点由下一次 run 接续。
 */
export type QueueItem = { id: number, text: MessageText, anchor: number | null, createdAt: number, pendingSteer: boolean, };
```

## RegisterNodeRequest

```ts
export type RegisterNodeRequest = { name: string, url: string, };
```

## RegisterWorkspaceRequest

```ts
export type RegisterWorkspaceRequest = { path: string, };
```

## RemoveWorkspaceRequest

```ts
export type RemoveWorkspaceRequest = { path: string, };
```

## RenameSessionRequest

```ts
export type RenameSessionRequest = { title: string, };
```

## ResyncReason

```ts
/**
 * 服务端要求客户端重连并刷新快照的原因。
 */
export type ResyncReason = { "kind": "lagged", skipped: number, };
```

## RunFailure

```ts
/**
 * run 失败类别与人类可读说明。
 */
export type RunFailure = { code: RunFailureCode, message: string, };
```

## RunFailureCode

```ts
/**
 * run 失败的稳定类别。
 */
export type RunFailureCode = "model_upstream" | "model_rate_limited" | "context_limit_exceeded" | "step_limit_exceeded" | "host_unreachable" | "storage_failed" | "interrupted";
```

## RunRecord

```ts
/**
 * 单个 run 的持久状态。
 */
export type RunRecord = { id: number, sessionId: SessionId, status: RunStatus, failure: RunFailure | null, 
/**
 * 流式生成中已经产生的正文。只在 run 为 `running` 时可能非空，结束或定型时清空。
 */
progress: string | null, createdAt: number, updatedAt: number, };
```

## RunStatus

```ts
/**
 * run 的生命周期状态。
 */
export type RunStatus = "running" | "succeeded" | "failed" | "cancelled";
```

## SessionId

```ts
/**
 * session 身份。
 */
export type SessionId = string;
```

## SessionMetadata

```ts
/**
 * 对外暴露的 session 元数据。
 */
export type SessionMetadata = { id: SessionId, workspace: WorkspacePath, title: SessionTitle, busy: boolean, queueLength: number, updatedAt: number, };
```

## SessionTitle

```ts
/**
 * session 标题。构造时去掉首尾空白，因此总是非空。
 */
export type SessionTitle = string;
```

## SessionTreeSnapshot

```ts
/**
 * 单个 session 的消息树快照。
 */
export type SessionTreeSnapshot = { session: SessionMetadata, 
/**
 * 快照读取时刻的全局最大事实事件 id；快照包含该 id 及之前的全部事实。
 */
maxEventId: number, activeLeaf: number | null, nodes: Array<MessageSkeleton>, queue: Array<QueueItem>, runs: Array<RunRecord>, };
```

## StreamEvent

```ts
/**
 * 事件流的统一载荷：已持久化的事实事件与仅实时传输的 token 增量。
 */
export type StreamEvent = { "type": "fact", id: number, sessionId: SessionId | null, 
/**
 * 事实提交后的 session 视图；不属于任何 session 或删除 session 时为空。
 */
session: SessionMetadata | null, timestamp: number, event: FactEvent, } | { "type": "delta", sessionId: SessionId, 
/**
 * 该步定型后使用的消息 id；客户端据此建本地占位节点。
 */
messageId: number, kind: DeltaKind, text: string, timestamp: number, } | { "type": "retry", sessionId: SessionId, 
/**
 * 即将进行的重试序号，从 1 开始。
 */
attempt: number, 
/**
 * 重试次数上限。
 */
maxAttempts: number, 
/**
 * 本次等待时长。
 */
delayMs: number, } | { "type": "resync", reason: ResyncReason, };
```

## SubmitMessageRequest

```ts
export type SubmitMessageRequest = { text: string, 
/**
 * 必填。省略时请求被拒绝。
 */
mode: SubmitMode, };
```

## SubmitMode

```ts
/**
 * 提交用户消息的语义。
 *
 * 没有缺省值：提交请求必须写明是排队还是插话。
 */
export type SubmitMode = "queue" | "steer";
```

## SwitchActiveLeafRequest

```ts
export type SwitchActiveLeafRequest = { messageId: number, };
```

## ToolErrorCode

```ts
/**
 * 工具失败的稳定类别。
 */
export type ToolErrorCode = "unknown_tool" | "invalid_arguments" | "path_escape" | "file_not_found" | "not_utf8" | "read_limit_exceeded" | "invalid_line_range" | "parent_not_found" | "edit_not_found" | "edit_not_unique" | "bash_timeout" | "io_error" | "link_closed" | "cancelled";
```

## ToolOutcome

```ts
/**
 * 工具执行返回的结局。
 */
export type ToolOutcome = { "status": "succeeded", content: string, } | { "status": "failed", code: ToolErrorCode, message: string, };
```

## ToolResultState

```ts
/**
 * tool 节点里工具结果块的状态。
 *
 * 工具在执行前入树，此时为 [`ToolResultState::Running`]；执行完毕写入结局；
 * 非幂等工具在执行中崩溃后写 [`ToolResultState::Unknown`]，表示丢失了观测。
 */
export type ToolResultState = { "status": "running" } | { "status": "finished", outcome: ToolOutcome, } | { "status": "unknown" };
```

## UpdateNodeRequest

```ts
export type UpdateNodeRequest = { 
/**
 * 改名后的节点名。省略表示保留原名。
 */
name: string | null, 
/**
 * 新的上游基地址。省略表示保留原 URL。
 */
url: string | null, 
/**
 * 是否启用上游订阅。省略表示保留原状态。
 */
enabled: boolean | null, };
```

## WorkspaceMetadata

```ts
export type WorkspaceMetadata = { path: WorkspacePath, registeredAt: number, };
```

## WorkspacePath

```ts
/**
 * workspace 身份：规范化后的绝对路径。
 */
export type WorkspacePath = string;
```

