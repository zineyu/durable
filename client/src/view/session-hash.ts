import type { NodeName } from '../api/generated/NodeName';
import { toSessionId } from './ids';
import type { SessionId } from './ids';

/// URL 指向的位置。`sessionId` 为 `null` 表示只选定了节点。
export type SessionLocation = {
  node: NodeName;
  sessionId: SessionId | null;
};

const nodeName = /^[A-Za-z0-9._-]{1,64}$/;
const location = /^#\/n\/([^/]+)(?:\/s\/([A-Za-z0-9._~-]+))?$/;

export function readLocationFromHash(hash: string): SessionLocation | null {
  const match = location.exec(hash);
  if (match === null) {
    return null;
  }
  const node = match[1];
  if (node === undefined || !nodeName.test(node) || node === '.' || node === '..') {
    return null;
  }
  const sessionId = match[2];
  return { node, sessionId: sessionId === undefined ? null : toSessionId(sessionId) };
}

export function writeLocationToHash(node: NodeName, sessionId: SessionId | null): string {
  return sessionId === null ? `#/n/${node}` : `#/n/${node}/s/${sessionId}`;
}
