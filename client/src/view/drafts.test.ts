import { describe, expect, it } from 'vitest';

import { toSessionId } from './ids';
import {
  draftKey,
  emptyDraft,
  readDraft,
  readLastSession,
  writeDraft,
  writeLastSession,
} from './drafts';

function storage(): Pick<Storage, 'getItem' | 'setItem' | 'removeItem'> & { entries: Map<string, string> } {
  const entries = new Map<string, string>();
  return {
    entries,
    getItem: (key) => entries.get(key) ?? null,
    setItem: (key, value) => {
      entries.set(key, value);
    },
    removeItem: (key) => {
      entries.delete(key);
    },
  };
}

const nodeA = 'macbook-air-01';
const nodeB = 'studio';
const sessionA = toSessionId('s-1');
const sessionB = toSessionId('s-2');

describe('草稿', () => {
  it('按节点与 session 隔离', () => {
    const store = storage();
    writeDraft(store, nodeA, sessionA, { text: '节点 A 的草稿', pastes: {} });
    writeDraft(store, nodeA, sessionB, { text: '另一个 session', pastes: {} });
    writeDraft(store, nodeB, sessionA, { text: '节点 B 的草稿', pastes: {} });

    expect(readDraft(store, nodeA, sessionA).text).toBe('节点 A 的草稿');
    expect(readDraft(store, nodeA, sessionB).text).toBe('另一个 session');
    expect(readDraft(store, nodeB, sessionA).text).toBe('节点 B 的草稿');
    expect(draftKey(nodeA, sessionA)).not.toBe(draftKey(nodeB, sessionA));
  });

  it('写入空文本时移除键', () => {
    const store = storage();
    writeDraft(store, nodeA, sessionA, { text: '要丢弃', pastes: {} });
    writeDraft(store, nodeA, sessionA, emptyDraft);
    expect(readDraft(store, nodeA, sessionA)).toEqual(emptyDraft);
    expect(store.entries.has(draftKey(nodeA, sessionA))).toBe(false);
  });

  it('折叠的粘贴块随草稿一起保存', () => {
    const store = storage();
    const pastes = { '[粘贴内容 1：900 字符]': 'x'.repeat(900) };
    writeDraft(store, nodeA, sessionA, { text: '看这个 [粘贴内容 1：900 字符]', pastes });
    expect(readDraft(store, nodeA, sessionA)).toEqual({ text: '看这个 [粘贴内容 1：900 字符]', pastes });
  });

  it('兼容旧格式的纯文本草稿', () => {
    const store = storage();
    store.setItem(draftKey(nodeA, sessionA), '旧草稿');
    expect(readDraft(store, nodeA, sessionA)).toEqual({ text: '旧草稿', pastes: {} });
  });

  it('没有选中 session 时返回空草稿', () => {
    const store = storage();
    expect(readDraft(store, null, null)).toEqual(emptyDraft);
    writeDraft(store, null, null, { text: '不写', pastes: {} });
    expect(store.entries.size).toBe(0);
  });

  it('每节点只记一个上次打开的 session', () => {
    const store = storage();
    expect(readLastSession(store, nodeA)).toBeNull();
    writeLastSession(store, nodeA, sessionA);
    writeLastSession(store, nodeB, sessionB);
    expect(readLastSession(store, nodeA)).toBe(sessionA);
    expect(readLastSession(store, nodeB)).toBe(sessionB);
  });
});
