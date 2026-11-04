import { describe, expect, it } from 'vitest';

import { toSessionId } from './ids';
import { memoryStorage } from './storage.fixture';
import {
  draftKey,
  emptyDraft,
  readDraft,
  readLastSession,
  writeDraft,
  writeLastSession,
} from './drafts';

const sessionA = toSessionId('s-1');
const sessionB = toSessionId('s-2');

describe('草稿', () => {
  it('按 session id 隔离', () => {
    const store = memoryStorage();
    writeDraft(store, sessionA, { text: '第一个 session 的草稿', pastes: {} });
    writeDraft(store, sessionB, { text: '另一个 session', pastes: {} });

    expect(readDraft(store, sessionA).text).toBe('第一个 session 的草稿');
    expect(readDraft(store, sessionB).text).toBe('另一个 session');
    expect(draftKey(sessionA)).not.toBe(draftKey(sessionB));
  });

  it('写入空文本时移除键', () => {
    const store = memoryStorage();
    writeDraft(store, sessionA, { text: '要丢弃', pastes: {} });
    writeDraft(store, sessionA, emptyDraft);
    expect(readDraft(store, sessionA)).toEqual(emptyDraft);
    expect(store.entries.has(draftKey(sessionA))).toBe(false);
  });

  it('折叠的粘贴块随草稿一起保存', () => {
    const store = memoryStorage();
    const pastes = { '[粘贴内容 1：900 字符]': 'x'.repeat(900) };
    writeDraft(store, sessionA, { text: '看这个 [粘贴内容 1：900 字符]', pastes });
    expect(readDraft(store, sessionA)).toEqual({ text: '看这个 [粘贴内容 1：900 字符]', pastes });
  });

  it('兼容旧格式的纯文本草稿', () => {
    const store = memoryStorage();
    store.setItem(draftKey(sessionA), '旧草稿');
    expect(readDraft(store, sessionA)).toEqual({ text: '旧草稿', pastes: {} });
  });

  it('没有选中 session 时返回空草稿', () => {
    const store = memoryStorage();
    expect(readDraft(store, null)).toEqual(emptyDraft);
    writeDraft(store, null, { text: '不写', pastes: {} });
    expect(store.entries.size).toBe(0);
  });

  it('只记一条上次打开的 session', () => {
    const store = memoryStorage();
    expect(readLastSession(store)).toBeNull();
    writeLastSession(store, sessionA);
    expect(readLastSession(store)).toBe(sessionA);
    writeLastSession(store, sessionB);
    expect(readLastSession(store)).toBe(sessionB);
    expect(store.entries.size).toBe(1);
  });
});
