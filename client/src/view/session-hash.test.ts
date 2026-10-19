import { describe, expect, it } from 'vitest';

import { toSessionId } from './ids';
import { readSessionFromHash, writeSessionToHash } from './session-hash';

describe('URL hash 中的 session', () => {
  it('读取合法 hash', () => {
    expect(readSessionFromHash('#session=01a15243-d0a9-7574-8e5c-a86a35aceb6c')).toBe(
      '01a15243-d0a9-7574-8e5c-a86a35aceb6c',
    );
  });

  it('空与非法 hash 返回 null', () => {
    for (const hash of ['', '#', '#session=', '#other=1', '#session=a b', '#session=a/b']) {
      expect(readSessionFromHash(hash), hash).toBeNull();
    }
  });

  it('写出 session id', () => {
    expect(writeSessionToHash(toSessionId('s-1'))).toBe('#session=s-1');
  });
});
