import type { HostName } from '../api/generated/HostName';
import type { RunStatus } from '../api/generated/RunStatus';
import { runStatusLabel, sessionStateLabel } from './labels';
import type { SessionState } from './types';

export type TitleSession = {
  host: HostName;
  title: string;
  state: SessionState;
  lastRun: RunStatus | null;
};

/// 页面标题反映当前 session 与运行状态，多标签页时便于分辨。
export function documentTitle(session: TitleSession | null): string {
  if (session === null) {
    return 'durable';
  }
  return `${statePrefix(session)}${session.title} · ${session.host}`;
}

function statePrefix(session: TitleSession): string {
  if (session.state === 'running') {
    return `${sessionStateLabel.running} · `;
  }
  if (session.state === 'queued') {
    return `${sessionStateLabel.queued} · `;
  }
  if (session.lastRun === 'failed') {
    return `${runStatusLabel.failed} · `;
  }
  if (session.lastRun === 'cancelled') {
    return `${runStatusLabel.cancelled} · `;
  }
  return '';
}
