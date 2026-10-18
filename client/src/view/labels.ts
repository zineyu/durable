import type { ConnectionState, MessageState, SessionState, StatusTone, ToolState } from './types';

export const sessionStateLabel: Record<SessionState, string> = {
  idle: '空闲',
  queued: '排队中',
  running: '执行中',
};

export const messageStateLabel: Record<MessageState, string> = {
  complete: '',
  streaming: '生成中',
  cancelled: '已取消',
  failed: '失败',
};

export const toolStateLabel: Record<ToolState, string> = {
  running: '执行中',
  succeeded: '成功',
  failed: '失败',
};

export const sessionStateTone: Record<SessionState, StatusTone> = {
  idle: 'idle',
  queued: 'queued',
  running: 'running',
};

export const connectionTone: Record<ConnectionState, StatusTone> = {
  connected: 'success',
  reconnecting: 'warn',
  disconnected: 'danger',
};

export const toolStateTone: Record<ToolState, StatusTone> = {
  running: 'running',
  succeeded: 'success',
  failed: 'danger',
};
