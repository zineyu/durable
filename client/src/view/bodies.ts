import type { ContentPart } from '../api/generated/ContentPart';
import type { DeltaKind } from '../api/generated/DeltaKind';
import type { MessageBody } from '../api/generated/MessageBody';
import type { ToolResultState } from '../api/generated/ToolResultState';
import type { NodeState } from './node-state';
import { transcriptNodes } from './messages';

/// 尚未定型的本地占位。`id` 是该步定型后将使用的消息 id。
export type MessagePlaceholder = {
  id: number;
  parts: ContentPart[];
};

/// 累加一次实时增量。定型前的输出不进树，因此这里只写本地占位。
export function applyDelta(
  state: NodeState,
  sessionId: string,
  messageId: number,
  kind: DeltaKind,
  text: string,
): NodeState {
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
export function requestMessageBodies(state: NodeState, ids: number[]): NodeState {
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
export function failMessageBodies(state: NodeState, ids: number[]): NodeState {
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
export function retryMessageBodies(state: NodeState, ids: number[]): NodeState {
  const failures = state.bodyFailures.filter((id) => !ids.includes(id));
  return failures.length === state.bodyFailures.length
    ? state
    : { ...state, bodyFailures: failures };
}

/// 写入批量拉取的正文。只接受仍在在途列表里的 id：已失效或快照重载丢弃的响应不写缓存。
export function applyMessageBodies(
  state: NodeState,
  ids: number[],
  bodies: MessageBody[],
): NodeState {
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
export function forgetBodies(state: NodeState, ids: number[]): NodeState {
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

/// 尚未取得正文、也没有在请求中的消息 id。调用方据此批量拉取。
export function missingBodyIds(state: NodeState, sessionId: string): number[] {
  return transcriptNodes(state, sessionId)
    .map((node) => node.id)
    .filter(
      (id) =>
        state.bodies[id] === undefined &&
        !state.bodyRequests.includes(id) &&
        !state.bodyFailures.includes(id),
    );
}

/// 一条消息的块正文；尚未拉到时为 `null`。
export function bodyOf(state: NodeState, messageId: number): ContentPart[] | null {
  return state.bodies[messageId] ?? null;
}

/// 按 call id 汇总工具结局。正文未到时没有条目，调用方按「执行中」渲染。
export function toolResults(state: NodeState, sessionId: string): Record<string, ToolResultState> {
  const tree = state.trees[sessionId];
  if (tree === undefined) {
    return {};
  }
  const results: Record<string, ToolResultState> = {};
  for (const node of tree.nodes) {
    if (node.role !== 'tool') {
      continue;
    }
    for (const part of state.bodies[node.id] ?? []) {
      if (part.kind === 'tool_result') {
        results[part.callId] = part.state;
      }
    }
  }
  return results;
}

/// 该 session 里尚未定型的占位，渲染顺序即产生顺序。
export function placeholdersOf(state: NodeState, sessionId: string): MessagePlaceholder[] {
  return state.placeholders[sessionId] ?? [];
}

/// 丢掉该消息的正文缓存并取消在途请求，使下一次渲染重新拉取。
export function invalidateBody(state: NodeState, id: number): NodeState {
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
export function dropPlaceholder(state: NodeState, sessionId: string, id: number): NodeState {
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
export function dropFinished(state: NodeState, sessionId: string): NodeState {
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
