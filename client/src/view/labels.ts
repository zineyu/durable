import type { HostStatus } from '../api/generated/HostStatus';
import type { RunFailureCode } from '../api/generated/RunFailureCode';
import type {
  ConnectionState,
  HostRecord,
  MessageState,
  RetryState,
  SessionState,
  StatusTone,
  ToolState,
} from './types';

export const hostStatusLabel: Record<HostStatus, string> = {
  ok: '可用',
  unreachable: '不可达',
  incompatible: '版本不兼容',
};

export const hostStatusTone: Record<HostStatus, StatusTone> = {
  ok: 'success',
  unreachable: 'danger',
  incompatible: 'warn',
};

export function sessionStateOf(busy: boolean, queueLength: number): SessionState {
  if (busy) {
    return 'running';
  }
  return queueLength > 0 ? 'queued' : 'idle';
}

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
  interrupted: '已中断',
};

export const toolStateLabel: Record<ToolState, string> = {
  running: '执行中',
  succeeded: '成功',
  failed: '失败',
  unknown: '结果未知',
};

export const runFailureLabel: Record<RunFailureCode, string> = {
  model_upstream: '模型上游错误',
  model_rate_limited: '供应商限流',
  context_limit_exceeded: '上下文超限',
  step_limit_exceeded: '步数超限',
  storage_failed: '本地存储失败',
  host_unreachable: '主机不可达',
  interrupted: '进程中断',
};

export function retryNote(retry: RetryState): string {
  return `供应商限流，第 ${retry.attempt}/${retry.maxAttempts} 次重试，等待 ${retryDelay(retry.delayMs)}`;
}

/// 侧栏与主面板里的 host 说明。host 可用时没有说明。
export function hostNotice(record: HostRecord | undefined): string | null {
  if (record === undefined) {
    return '未知 host，host 表里没有这个名字';
  }
  if (record.status === 'ok') {
    return null;
  }
  return record.detail ?? hostStatusLabel[record.status];
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
  unknown: 'warn',
};
