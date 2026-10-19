import { toSessionId } from './ids';
import type { SessionId } from './ids';

const sessionHash = /^#session=([A-Za-z0-9._~-]+)$/;

export function readSessionFromHash(hash: string): SessionId | null {
  const match = sessionHash.exec(hash);
  if (match === null) {
    return null;
  }
  const value = match[1];
  if (value === undefined) {
    return null;
  }
  return toSessionId(value);
}

export function writeSessionToHash(id: SessionId): string {
  return `#session=${id}`;
}
