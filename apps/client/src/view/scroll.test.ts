import { describe, expect, it } from 'vitest';

import { toSessionId } from './ids';
import { readScroll, scrollKey, writeScroll } from './scroll';
import { memoryStorage } from './storage.fixture';

const sessionA = toSessionId('s-1');
const sessionB = toSessionId('s-2');

describe('transcript 滚动位置', () => {
  it('按 session id 隔离', () => {
    const store = memoryStorage();
    writeScroll(store, sessionA, 420);
    writeScroll(store, sessionB, 40);

    expect(readScroll(store, sessionA)).toBe(420);
    expect(readScroll(store, sessionB)).toBe(40);
    expect(scrollKey(sessionA)).not.toBe(scrollKey(sessionB));
  });

  it('没有记录、记录损坏与顶部位置都读作 0', () => {
    const store = memoryStorage();
    expect(readScroll(store, sessionA)).toBe(0);

    writeScroll(store, sessionA, 120);
    store.setItem(scrollKey(sessionA), 'bottom');
    expect(readScroll(store, sessionA)).toBe(0);

    store.setItem(scrollKey(sessionA), '-8');
    expect(readScroll(store, sessionA)).toBe(0);
  });

  it('回到顶部时删除记录，位置取整数', () => {
    const store = memoryStorage();
    writeScroll(store, sessionA, 120.6);
    expect(readScroll(store, sessionA)).toBe(121);

    writeScroll(store, sessionA, 0);
    expect(store.entries.has(scrollKey(sessionA))).toBe(false);
  });
});
