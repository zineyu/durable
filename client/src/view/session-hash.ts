import { toSessionId } from './ids';
import type { SessionId } from './ids';

/// 选中的 session。host 由 session 的 workspace 推出，因此 URL 里没有它。
/// 规格 user story R-44：URL 形如 `#/s/<session_id>`。
export type Selection = SessionId | null;

const sessionLocation = /^#\/s\/([A-Za-z0-9._~-]+)$/;

/// 从哈希读出选中 session。初始化与 `hashchange` 共用本函数。
export function readSelection(hash: string): Selection {
  const match = sessionLocation.exec(hash);
  if (match === null) {
    return null;
  }
  const id = match[1];
  return id === undefined ? null : toSessionId(id);
}

export function writeSelection(sessionId: Selection): string {
  return sessionId === null ? '' : `#/s/${sessionId}`;
}
