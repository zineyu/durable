import type { NodeName } from '../api/generated/NodeName';
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
const LAST_SESSION_PREFIX = 'durable:last-session';

export function draftKey(node: NodeName, sessionId: SessionId): string {
  return `${DRAFT_PREFIX}:${node}:${sessionId}`;
}

function lastSessionKey(node: NodeName): string {
  return `${LAST_SESSION_PREFIX}:${node}`;
}

export function readDraft(
  storage: DraftStorage,
  node: NodeName | null,
  sessionId: SessionId | null,
): Draft {
  if (node === null || sessionId === null) {
    return emptyDraft;
  }
  const raw = storage.getItem(draftKey(node, sessionId));
  if (raw === null) {
    return emptyDraft;
  }
  return parseDraft(raw);
}

export function writeDraft(
  storage: DraftStorage,
  node: NodeName | null,
  sessionId: SessionId | null,
  draft: Draft,
): void {
  if (node === null || sessionId === null) {
    return;
  }
  const key = draftKey(node, sessionId);
  if (draft.text.length === 0 && Object.keys(draft.pastes).length === 0) {
    storage.removeItem(key);
    return;
  }
  storage.setItem(key, JSON.stringify(draft));
}

export function readLastSession(storage: DraftStorage, node: NodeName): SessionId | null {
  const raw = storage.getItem(lastSessionKey(node));
  if (raw === null || raw.trim().length === 0) {
    return null;
  }
  return toSessionId(raw);
}

export function writeLastSession(
  storage: DraftStorage,
  node: NodeName,
  sessionId: SessionId,
): void {
  storage.setItem(lastSessionKey(node), sessionId);
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
