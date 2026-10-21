# 协议类型清单

由 `devenv shell -- durable-typegen` 从 Rust 类型生成，与 `client/src/api/generated/` 同一来源。不要手改。

## BootstrapResponse

```ts
export type BootstrapResponse = { model: string, maxEventId: number, workspaces: Array<WorkspaceMetadata>, sessions: Array<SessionMetadata>, };
```

## CreateSessionRequest

```ts
export type CreateSessionRequest = { workspace: string, title: string | null, };
```

## EditMessageRequest

```ts
export type EditMessageRequest = { text: string, };
```

## ErrorCode

```ts
export type ErrorCode = "workspace_path_invalid" | "workspace_duplicate" | "workspace_not_found" | "workspace_not_empty" | "session_not_found" | "queue_item_not_found" | "message_not_found" | "message_role_mismatch" | "run_in_progress" | "queue_not_empty" | "invalid_title" | "invalid_message" | "invalid_request" | "internal";
```

## ErrorResponse

```ts
export type ErrorResponse = { code: ErrorCode, message: string, };
```

## FactEvent

```ts
/**
 * 持久化到 SQLite 的领域事实事件。
 */
export type FactEvent = { "kind": "workspace_registered", workspace: WorkspaceMetadata, } | { "kind": "workspace_removed", path: WorkspacePath, } | { "kind": "session_created", session: SessionMetadata, } | { "kind": "session_renamed", session: SessionMetadata, } | { "kind": "session_deleted", sessionId: SessionId, } | { "kind": "message_appended", session: SessionMetadata, message: MessageNode, } | { "kind": "message_updated", session: SessionMetadata, message: MessageNode, } | { "kind": "tool_call_recorded", session: SessionMetadata, message: number, toolCall: ToolCallRecord, } | { "kind": "active_leaf_changed", session: SessionMetadata, activeLeaf: number | null, } | { "kind": "message_queued", session: SessionMetadata, item: QueueItem, } | { "kind": "queue_item_removed", sessionId: SessionId, itemId: number, } | { "kind": "run_started", session: SessionMetadata, run: RunRecord, } | { "kind": "run_finished", session: SessionMetadata, run: RunRecord, };
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
export type HubErrorCode = "node_name_invalid" | "node_name_taken" | "unknown_node" | "node_unavailable" | "invalid_request" | "internal";
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
export type HubStreamEvent = { "type": "fact", hubSeq: number, node: NodeName, id: number, sessionId: SessionId | null, timestamp: number, event: FactEvent, } | { "type": "delta", node: NodeName, sessionId: SessionId, messageId: number, text: string, timestamp: number, } | { "type": "retry", node: NodeName, sessionId: SessionId, 
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
delayMs: number, } | { "type": "node_registered", hubSeq: number, node: NodeMetadata, } | { "type": "node_removed", hubSeq: number, name: NodeName, } | { "type": "node_status_changed", hubSeq: number, name: NodeName, status: NodeStatus, detail: string | null, } | { "type": "resync", reason: ResyncReason, };
```

## MessageContent

```ts
/**
 * 消息节点正文。用户消息经 [`MessageText`] 校验；assistant 消息可以为空（只请求工具时）。
 */
export type MessageContent = string;
```

## MessageNode

```ts
/**
 * 消息树节点，同时是协议中的消息形状。
 */
export type MessageNode = { id: number, parentId: number | null, role: MessageRole, text: MessageContent, runId: number | null, toolCalls: Array<ToolCallRecord>, createdAt: number, };
```

## MessageRole

```ts
export type MessageRole = "user" | "assistant";
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
export type NodeMetadata = { name: NodeName, url: string, createdAt: number, };
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
export type NodeSnapshot = { name: NodeName, url: string, status: NodeStatus, detail: string | null, bootstrap: BootstrapResponse | null, };
```

## NodeStatus

```ts
/**
 * 节点状态。只有 `ok` 的节点接受命令。
 */
export type NodeStatus = "ok" | "unreachable" | "incompatible";
```

## QueueItem

```ts
/**
 * 等待执行的用户消息。session 级 FIFO，按 id 升序执行。
 *
 * `anchor` 是本次执行接续的用户节点。`None` 表示 run 开始时在当前 active leaf 下新建用户节点；
 * `Some` 表示接续已存在的用户节点（编辑产生的新节点或重试 assistant 的父节点）。
 */
export type QueueItem = { id: number, text: MessageText, anchor: number | null, createdAt: number, };
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

## RenameNodeRequest

```ts
export type RenameNodeRequest = { name: string, };
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
export type RunFailureCode = "model_upstream" | "model_rate_limited" | "context_limit_exceeded" | "step_limit_exceeded" | "interrupted";
```

## RunRecord

```ts
/**
 * 单个 run 的持久状态。
 */
export type RunRecord = { id: number, sessionId: SessionId, status: RunStatus, failure: RunFailure | null, createdAt: number, updatedAt: number, };
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

## SessionTree

```ts
/**
 * 单个 session 的消息树快照。
 */
export type SessionTree = { session: SessionMetadata, 
/**
 * 快照读取时刻的全局最大事实事件 id；快照包含该 id 及之前的全部事实。
 */
maxEventId: number, activeLeaf: number | null, nodes: Array<MessageNode>, queue: Array<QueueItem>, runs: Array<RunRecord>, };
```

## StreamEvent

```ts
/**
 * 事件流的统一载荷：已持久化的事实事件与仅实时传输的 token 增量。
 */
export type StreamEvent = { "type": "fact", id: number, sessionId: SessionId | null, timestamp: number, event: FactEvent, } | { "type": "delta", sessionId: SessionId, messageId: number, text: string, timestamp: number, } | { "type": "retry", sessionId: SessionId, 
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
export type SubmitMessageRequest = { text: string, };
```

## SwitchActiveLeafRequest

```ts
export type SwitchActiveLeafRequest = { messageId: number, };
```

## ToolCallRecord

```ts
/**
 * assistant 消息发起的单个工具调用及其结果。
 */
export type ToolCallRecord = { id: string, name: string, arguments: string, outcome: ToolOutcome, };
```

## ToolErrorCode

```ts
/**
 * 工具失败的稳定类别。
 */
export type ToolErrorCode = "unknown_tool" | "invalid_arguments" | "path_escape" | "file_not_found" | "not_utf8" | "read_limit_exceeded" | "invalid_line_range" | "parent_not_found" | "edit_not_found" | "edit_not_unique" | "bash_timeout" | "io_error" | "cancelled";
```

## ToolOutcome

```ts
/**
 * 工具调用的结局。
 */
export type ToolOutcome = { "status": "succeeded", content: string, } | { "status": "failed", code: ToolErrorCode, message: string, };
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

