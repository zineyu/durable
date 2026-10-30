/// 本模块负责未定型占位的状态迁移：流式增量合并、定型丢弃与 run 结束丢弃。
/// 已定型的正文缓存与拉取在 `body-cache.ts`；读取占位给呈现层的是 `derived.ts`。
import type { ContentPart } from '../api/generated/ContentPart';
import type { DeltaKind } from '../api/generated/DeltaKind';
import type { ClientState } from './state';
import type { MessagePlaceholder } from './types';

/// 累加一次实时增量。定型前的输出不进树，因此这里只写本地占位。
export function applyDelta(
  state: ClientState,
  sessionId: string,
  messageId: number,
  kind: DeltaKind,
  text: string,
): ClientState {
  const tree = state.trees[sessionId];
  if (tree === undefined) {
    return state;
  }
  if (tree.nodes.some((node) => node.id === messageId)) {
    return state;
  }
  const placeholders = state.placeholders[sessionId] ?? [];
  const index = placeholders.findIndex((placeholder) => placeholder.id === messageId);
  const parts = index === -1 ? [] : (placeholders[index]?.parts ?? []);
  const next: MessagePlaceholder = { id: messageId, parts: appendDelta(parts, kind, text) };
  const updated =
    index === -1
      ? [...placeholders, next]
      : placeholders.map((placeholder, position) => (position === index ? next : placeholder));
  return {
    ...state,
    placeholders: { ...state.placeholders, [sessionId]: updated },
  };
}

/// 丢掉一个已定型的占位。
export function dropPlaceholder(state: ClientState, sessionId: string, id: number): ClientState {
  const current = state.placeholders[sessionId];
  if (current === undefined) {
    return state;
  }
  if (!current.some((placeholder) => placeholder.id === id)) {
    return state;
  }
  return {
    ...state,
    placeholders: {
      ...state.placeholders,
      [sessionId]: current.filter((placeholder) => placeholder.id !== id),
    },
  };
}

/// run 结束：未定型的输出不落库，占位一并丢弃。
export function dropFinished(state: ClientState, sessionId: string): ClientState {
  const current = state.placeholders[sessionId];
  if (current === undefined || current.length === 0) {
    return state;
  }
  return { ...state, placeholders: { ...state.placeholders, [sessionId]: [] } };
}

/// 把增量并进占位块序：同种相邻块合并，否则追加新块。
function appendDelta(parts: ContentPart[], kind: DeltaKind, text: string): ContentPart[] {
  const last = parts.at(-1);
  if (last?.kind === kind) {
    return [...parts.slice(0, -1), { ...last, text: last.text + text }];
  }
  return [...parts, { kind, text, providerMeta: null }];
}
