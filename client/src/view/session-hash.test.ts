import { describe, expect, it } from 'vitest';

import { toSessionId } from './ids';
import { readLocationFromHash, writeLocationToHash } from './session-hash';

describe('session 位置', () => {
  it('解析节点与 session', () => {
    expect(readLocationFromHash('#/n/macbook-air-01/s/s-1')).toEqual({
      node: 'macbook-air-01',
      sessionId: toSessionId('s-1'),
    });
  });

  it('只有节点时 session 为空', () => {
    expect(readLocationFromHash('#/n/studio')).toEqual({ node: 'studio', sessionId: null });
  });

  it('拒绝单节点时代的旧地址', () => {
    expect(readLocationFromHash('#session=s-1')).toBeNull();
  });

  it('拒绝非法节点名与多余路径', () => {
    expect(readLocationFromHash('#/n//s/s-1')).toBeNull();
    expect(readLocationFromHash('#/n/a%2Fb/s/s-1')).toBeNull();
    expect(readLocationFromHash('#/n/../s/s-1')).toBeNull();
    expect(readLocationFromHash('#/n/a.b_c-1/s/s-1')).not.toBeNull();
    expect(readLocationFromHash('#/n/node/s/s-1/extra')).toBeNull();
  });

  it('拒绝含斜杠的 session id', () => {
    expect(readLocationFromHash('#/n/node/s/a/b')).toBeNull();
  });

  it('空 hash 表示未选择任何位置', () => {
    expect(readLocationFromHash('')).toBeNull();
    expect(readLocationFromHash('#')).toBeNull();
  });

  it('写入与读取互逆', () => {
    expect(writeLocationToHash('studio', toSessionId('s-2'))).toBe('#/n/studio/s/s-2');
    expect(writeLocationToHash('studio', null)).toBe('#/n/studio');
    expect(readLocationFromHash(writeLocationToHash('studio', toSessionId('s-2')))).toEqual({
      node: 'studio',
      sessionId: toSessionId('s-2'),
    });
    expect(readLocationFromHash(writeLocationToHash('studio', null))).toEqual({
      node: 'studio',
      sessionId: null,
    });
  });
});
