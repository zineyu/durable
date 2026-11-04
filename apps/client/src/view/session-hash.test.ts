import { describe, expect, it } from 'vitest';

import { toSessionId } from './ids';
import { readSelection, writeSelection } from './session-hash';

describe('session 位置', () => {
  it('从哈希读出 session id', () => {
    expect(readSelection('#/s/s-1')).toBe(toSessionId('s-1'));
    expect(readSelection('#/s/01a1813b-4f63-70b8-bd3e')).toBe(
      toSessionId('01a1813b-4f63-70b8-bd3e'),
    );
  });

  it('拒绝多余的路径前缀与单 session 参数地址', () => {
    expect(readSelection('#/h/studio/s/s-1')).toBeNull();
    expect(readSelection('#session=s-1')).toBeNull();
  });

  it('拒绝空 id、斜杠与多余路径', () => {
    expect(readSelection('#/s/')).toBeNull();
    expect(readSelection('#/s/a/b')).toBeNull();
    expect(readSelection('#/s/a%2Fb')).toBeNull();
    expect(readSelection('#/s/../x')).toBeNull();
    expect(readSelection('#/s/s-1/extra')).toBeNull();
  });

  it('空 hash 表示未选择 session', () => {
    expect(readSelection('')).toBeNull();
    expect(readSelection('#')).toBeNull();
  });

  it('写入与读取互逆', () => {
    expect(writeSelection(toSessionId('s-2'))).toBe('#/s/s-2');
    expect(writeSelection(null)).toBe('');
    expect(readSelection(writeSelection(toSessionId('s-2')))).toBe(toSessionId('s-2'));
    expect(readSelection(writeSelection(null))).toBeNull();
  });
});
