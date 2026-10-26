import type { ContentPart } from '../api/generated/ContentPart';
import type { MessageSkeleton } from '../api/generated/MessageSkeleton';
import type { NodeName } from '../api/generated/NodeName';
import type { NodeStatus } from '../api/generated/NodeStatus';
import type { RunStatus } from '../api/generated/RunStatus';
import type { SessionMetadata } from '../api/generated/SessionMetadata';
import type { ToolResultState } from '../api/generated/ToolResultState';
import type { QueueItemId, SessionId, WorkspaceId, WorkspacePath } from './ids';

export type SessionState = 'idle' | 'queued' | 'running';

/// 最近一次 run 的结局。取消与进行中不给出结局。
export type SessionOutcome = 'succeeded' | 'failed';

export type ConnectionState = 'connected' | 'reconnecting' | 'disconnected';

export type StatusTone = 'idle' | 'running' | 'queued' | 'success' | 'warn' | 'danger';

export type IconButtonTone = 'neutral' | 'danger' | 'primary';

export type IconButtonSize = 'sm' | 'md' | 'lg';

export type MessageState = 'complete' | 'streaming' | 'cancelled' | 'failed';

/// 限流重试的一次等待。仅实时：不落库，刷新后不存在。
export type RetryState = {
  attempt: number;
  maxAttempts: number;
  delayMs: number;
};

export type ToolState = 'running' | 'succeeded' | 'failed' | 'unknown';

export type SessionSummary = {
  id: SessionId;
  title: string;
  state: SessionState;
  queuedCount: number;
  updatedAt: number;
  /// 上次查看之后 session 又更新过。
  unread: boolean;
  /// 最近一次 run 的结局。取消与进行中不给出结局。
  outcome: SessionOutcome | null;
};

export type WorkspaceSummary = {
  id: WorkspaceId;
  path: WorkspacePath;
  sessions: SessionSummary[];
};

export type QueueItem = {
  id: QueueItemId;
  text: string;
  /// 该项是等待插入当前 run 的插话。
  pendingSteer: boolean;
};

export type BranchControl = {
  index: number;
  total: number;
  onPrev?: () => void;
  onNext?: () => void;
};

/// 兄弟分支位置。`index` 从 1 开始，`total` 最少为 2。
export type BranchPosition = {
  index: number;
  total: number;
  previousId: number | null;
  nextId: number | null;
};

/// 尚未定型的本地占位。`id` 是该步定型后将使用的消息 id。
export type MessagePlaceholder = {
  id: number;
  parts: ContentPart[];
};

/// 活动链上的一条消息，连同渲染所需的派生结果。
export type TranscriptEntry = {
  message: MessageSkeleton;
  /// 块正文；尚未拉到时为 `null`。
  parts: ContentPart[] | null;
  state: MessageState;
  /// 进行中或失败时的说明；其余为 `null`。
  note: string | null;
  /// 兄弟分支位置；没有分支时为 `null`。
  branch: BranchPosition | null;
};

/// 一个 session 的完整派生视图。主面板与 session 内搜索、导出都只读它。
export type SessionView = {
  /// session 元数据：标题、workspace 与忙碌标志。
  session: SessionMetadata;
  summary: SessionSummary;
  /// 活动链上的消息，按链上顺序，不含工具节点。
  transcript: TranscriptEntry[];
  /// 输入框回溯用的用户消息正文，按链上顺序。
  history: string[];
  /// 按 call id 汇总的工具结局；正文未到的调用没有条目。
  toolResults: Record<string, ToolResultState>;
  queue: QueueItem[];
  /// 尚未定型的占位，渲染在 transcript 末尾。
  placeholders: MessagePlaceholder[];
  /// 最近一次 run 的状态；没有 run 时为 `null`。页面标题据它加前缀。
  lastRun: RunStatus | null;
  /// 没有 run 在跑，且队列为空。
  idle: boolean;
};

/// 节点级摘要：模型名与按 workspace 分组的 session 摘要。
export type NodeSummary = {
  modelName: string;
  workspaces: WorkspaceSummary[];
};

/// 命令面板里的一项 session。节点名写明来源，跨节点列表靠它区分。
export type PaletteSession = {
  node: NodeName;
  sessionId: SessionId;
  title: string;
  workspace: string;
  updatedAt: number;
};

/// 跨节点运行面板的一项。
export type RunningEntry = {
  node: NodeName;
  sessionId: SessionId;
  title: string;
  queueLength: number;
  /// 当前 run 的开始时间；分片里还没有 run 记录时为 `null`。
  startedAt: number | null;
};

/// 节点表的一项。`detail` 是节点状态的具体说明。
export type NodeRecord = {
  name: NodeName;
  url: string;
  enabled: boolean;
  status: NodeStatus;
  detail: string | null;
};
