import type { QueueItemId, SessionId, WorkspaceId, WorkspacePath } from './ids';

export type SessionState = 'idle' | 'queued' | 'running';

export type ConnectionState = 'connected' | 'reconnecting' | 'disconnected';

export type StatusTone = 'idle' | 'running' | 'queued' | 'success' | 'warn' | 'danger';

export type IconButtonTone = 'neutral' | 'danger' | 'primary';

export type IconButtonSize = 'sm' | 'md' | 'lg';

export type MessageState = 'complete' | 'streaming' | 'cancelled' | 'failed';

export type ToolState = 'running' | 'succeeded' | 'failed';

export type SessionSummary = {
  id: SessionId;
  title: string;
  state: SessionState;
  queuedCount: number;
  updatedAt: number;
};

export type WorkspaceSummary = {
  id: WorkspaceId;
  path: WorkspacePath;
  sessions: SessionSummary[];
};

export type QueueItem = {
  id: QueueItemId;
  text: string;
};

export type BranchControl = {
  index: number;
  total: number;
  onPrev?: () => void;
  onNext?: () => void;
};
