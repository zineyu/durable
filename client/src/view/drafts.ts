import { toSessionId } from './ids';
import type { SessionId } from './ids';

/// 浏览器存储的最小接口。测试传入内存实现，生产传 `window.localStorage`。
export type DraftStorage = Pick<Storage, 'getItem' | 'setItem' | 'removeItem'>;

/// 一条 session 的草稿。折叠的粘贴块与正文一起保存，刷新后可还原。
export type Draft = {
  text: string;
  pastes: Record<string, string>;
};

export const emptyDraft: Draft = { text: '', pastes: {} };

const DRAFT_PREFIX = 'durable:draft';
const LAST_SESSION_KEY = 'durable:last-session';

export function draftKey(sessionId: SessionId): string {
  return `${DRAFT_PREFIX}:${sessionId}`;
}

export function readDraft(storage: DraftStorage, sessionId: SessionId | null): Draft {
  if (sessionId === null) {
    return emptyDraft;
  }
  const raw = storage.getItem(draftKey(sessionId));
  if (raw === null) {
    return emptyDraft;
  }
  return parseDraft(raw);
}

export function writeDraft(
  storage: DraftStorage,
  sessionId: SessionId | null,
  draft: Draft,
): void {
  if (sessionId === null) {
    return;
  }
  const key = draftKey(sessionId);
  if (draft.text.length === 0 && Object.keys(draft.pastes).length === 0) {
    storage.removeItem(key);
    return;
  }
  storage.setItem(key, JSON.stringify(draft));
}

/// 上次打开的 session。session id 全局唯一，因此所有 host 共用一条记录。
export function readLastSession(storage: DraftStorage): SessionId | null {
  const raw = storage.getItem(LAST_SESSION_KEY);
  if (raw === null || raw.trim().length === 0) {
    return null;
  }
  return toSessionId(raw);
}

export function writeLastSession(storage: DraftStorage, sessionId: SessionId): void {
  storage.setItem(LAST_SESSION_KEY, sessionId);
}

function parseDraft(raw: string): Draft {
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return { text: raw, pastes: {} };
  }
  if (typeof parsed !== 'object' || parsed === null) {
    return { text: raw, pastes: {} };
  }
  const text = 'text' in parsed ? parsed.text : undefined;
  if (typeof text !== 'string') {
    return { text: raw, pastes: {} };
  }
  const pastes = 'pastes' in parsed ? parsed.pastes : undefined;
  return { text, pastes: readPastes(pastes) };
}

function readPastes(value: unknown): Record<string, string> {
  if (typeof value !== 'object' || value === null) {
    return {};
  }
  const pastes: Record<string, string> = {};
  for (const [token, text] of Object.entries(value)) {
    if (typeof text === 'string') {
      pastes[token] = text;
    }
  }
  return pastes;
}
