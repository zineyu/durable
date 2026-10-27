import type { NodeName } from '../api/generated/NodeName';
import type { NodeStatus } from '../api/generated/NodeStatus';
import type { RunFailureCode } from '../api/generated/RunFailureCode';
import type {
  ConnectionState,
  MessageState,
  NodeRecord,
  RetryState,
  SessionState,
  StatusTone,
  ToolState,
} from './types';

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
  interrupted: '进程中断',
};

export function retryNote(retry: RetryState): string {
  return `供应商限流，第 ${retry.attempt}/${retry.maxAttempts} 次重试，等待 ${retryDelay(retry.delayMs)}`;
}

/// 节点切换器下方的说明。节点可用时没有说明。
export function unavailableNotice(
  selected: NodeName | null,
  record: NodeRecord | undefined,
): string | null {
  if (selected === null) {
    return null;
  }
  if (record === undefined) {
    return '未知节点，节点表里没有这个名字';
  }
  if (record.status === 'ok') {
    return null;
  }
  return record.detail ?? nodeStatusLabel[record.status];
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
