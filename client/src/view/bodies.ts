/// 本模块负责块正文缓存与未定型占位的状态迁移。读取这些状态给呈现层的是 `derived.ts`。
import type { ContentPart } from '../api/generated/ContentPart';
import type { DeltaKind } from '../api/generated/DeltaKind';
import type { MessageBody } from '../api/generated/MessageBody';
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

/// 标记这些消息的正文正在拉取。
export function requestMessageBodies(state: ClientState, ids: number[]): ClientState {
  const requests = [...state.bodyRequests];
  for (const id of ids) {
    if (!requests.includes(id)) {
      requests.push(id);
    }
  }
  return requests.length === state.bodyRequests.length
    ? state
    : { ...state, bodyRequests: requests };
}

/// 正文拉取失败：移出在途列表并记住失败，避免立即重试。
export function failMessageBodies(state: ClientState, ids: number[]): ClientState {
  const failures = [...state.bodyFailures];
  for (const id of ids) {
    if (!failures.includes(id)) {
      failures.push(id);
    }
  }
  return {
    ...state,
    bodyFailures: failures,
    bodyRequests: state.bodyRequests.filter((id) => !ids.includes(id)),
  };
}

/// 允许重试这些失败的消息。
export function retryMessageBodies(state: ClientState, ids: number[]): ClientState {
  const failures = state.bodyFailures.filter((id) => !ids.includes(id));
  return failures.length === state.bodyFailures.length
    ? state
    : { ...state, bodyFailures: failures };
}

/// 写入批量拉取的正文。只接受仍在在途列表里的 id：已失效或快照重载丢弃的响应不写缓存。
export function applyMessageBodies(
  state: ClientState,
  ids: number[],
  bodies: MessageBody[],
): ClientState {
  const accepted = ids.filter((id) => state.bodyRequests.includes(id));
  if (accepted.length === 0) {
    return state;
  }
  const loaded: Record<number, ContentPart[]> = { ...state.bodies };
  for (const body of bodies) {
    if (accepted.includes(body.id)) {
      loaded[body.id] = body.parts;
    }
  }
  for (const id of accepted) {
    loaded[id] ??= [];
  }
  return {
    ...state,
    bodies: loaded,
    bodyRequests: state.bodyRequests.filter((id) => !accepted.includes(id)),
  };
}

/// 丢弃这些消息的正文与请求记录，用于 session 删除。
export function forgetBodies(state: ClientState, ids: number[]): ClientState {
  if (ids.length === 0) {
    return state;
  }
  const keep = (key: string): boolean => !ids.includes(Number(key));
  return {
    ...state,
    bodies: Object.fromEntries(Object.entries(state.bodies).filter(([key]) => keep(key))),
    bodyRequests: state.bodyRequests.filter((id) => !ids.includes(id)),
    bodyFailures: state.bodyFailures.filter((id) => !ids.includes(id)),
  };
}

/// 丢掉该消息的正文缓存并取消在途请求，使下一次渲染重新拉取。
export function invalidateBody(state: ClientState, id: number): ClientState {
  const pending = state.bodyRequests.includes(id);
  const failed = state.bodyFailures.includes(id);
  if (state.bodies[id] === undefined && !pending && !failed) {
    return state;
  }
  return {
    ...state,
    bodies: Object.fromEntries(
      Object.entries(state.bodies).filter(([key]) => Number(key) !== id),
    ),
    bodyRequests: state.bodyRequests.filter((entry) => entry !== id),
    bodyFailures: state.bodyFailures.filter((entry) => entry !== id),
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
