import type { NodeStatus } from '../api/generated/NodeStatus';
import type { RunFailureCode } from '../api/generated/RunFailureCode';
import type { ConnectionState, MessageState, RetryState, SessionState, StatusTone, ToolState } from './types';

export const nodeStatusLabel: Record<NodeStatus, string> = {
  ok: '可用',
  unreachable: '不可达',
  incompatible: '版本不兼容',
};

export const nodeStatusTone: Record<NodeStatus, StatusTone> = {
  ok: 'success',
  unreachable: 'danger',
  incompatible: 'warn',
};

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

export const runFailureLabel: Record<RunFailureCode, string> = {
  model_upstream: '模型上游错误',
  model_rate_limited: '供应商限流',
  context_limit_exceeded: '上下文超限',
  step_limit_exceeded: '步数超限',
  interrupted: '进程中断',
};

export function retryNote(retry: RetryState): string {
  return `供应商限流，第 ${retry.attempt}/${retry.maxAttempts} 次重试，等待 ${retryDelay(retry.delayMs)}`;
}

function retryDelay(delayMs: number): string {
  if (delayMs < 1000) {
    return `${delayMs} 毫秒`;
  }
  return `${Number((delayMs / 1000).toFixed(1))} 秒`;
}

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
