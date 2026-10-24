import type { NodeName } from '../api/generated/NodeName';
import type { NodeStatus } from '../api/generated/NodeStatus';
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

/// 节点表的一项。`detail` 是节点状态的具体说明。
export type NodeRecord = {
  name: NodeName;
  url: string;
  status: NodeStatus;
  detail: string | null;
};
