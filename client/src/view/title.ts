import type { NodeName } from '../api/generated/NodeName';
import type { RunStatus } from '../api/generated/RunStatus';
import type { SessionState } from './types';

export type TitleSession = {
  node: NodeName;
  title: string;
  state: SessionState;
  lastRun: RunStatus | null;
};

/// 页面标题反映当前 session 与运行状态，多标签页时便于分辨。
export function documentTitle(session: TitleSession | null): string {
  if (session === null) {
    return 'durable';
  }
  return `${statePrefix(session)}${session.title} · ${session.node}`;
}

function statePrefix(session: TitleSession): string {
  if (session.state === 'running') {
    return '执行中 · ';
  }
  if (session.state === 'queued') {
    return '排队中 · ';
  }
  if (session.lastRun === 'failed') {
    return '失败 · ';
  }
  if (session.lastRun === 'cancelled') {
    return '已取消 · ';
  }
  return '';
}
