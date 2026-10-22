import type { BootstrapResponse } from '../api/generated/BootstrapResponse';
import type { ContentPart } from '../api/generated/ContentPart';
import type { FactEvent } from '../api/generated/FactEvent';
import type { MessageSkeleton } from '../api/generated/MessageSkeleton';
import type { QueueItem } from '../api/generated/QueueItem';
import type { RunRecord } from '../api/generated/RunRecord';
import type { SessionMetadata } from '../api/generated/SessionMetadata';
import type { SessionTree } from '../api/generated/SessionTree';
import type { WorkspaceMetadata } from '../api/generated/WorkspaceMetadata';
import { dropFinished, dropPlaceholder, forgetBodies, invalidateBody } from './bodies';
import type { MessagePlaceholder } from './bodies';
import type { RetryState } from './types';

/// 已应用的事实。快照可能比已应用的尾部旧，用它补齐快照水位之后的事实。
type AppliedFact = {
  id: number;
  event: FactEvent;
};

/// 每节点保留的已应用事实条数。快照与已应用尾部之间的差距远小于它。
const APPLIED_FACT_LIMIT = 128;

export type SessionTreeState = {
  activeLeaf: number | null;
  nodes: MessageSkeleton[];
  queue: QueueItem[];
  runs: RunRecord[];
};

/// 单个节点的分片。一个节点的事件只归约到自己的分片，分片之间互不影响。
export type NodeState = {
  modelName: string;
  workspaces: WorkspaceMetadata[];
  sessions: SessionMetadata[];
  trees: Record<string, SessionTreeState>;
  /// 按 session 记录的重试等待；仅实时，随进度清除。
  retries: Record<string, RetryState>;
  /// 按消息 id 缓存的块正文。分支切换会复用同一条消息，因此不能按位置缓存。
  bodies: Record<number, ContentPart[]>;
  /// 已发出、尚未收到响应的正文请求。
  bodyRequests: number[];
  /// 请求失败的消息 id。快照重载或该消息更新后清除，避免反复重试。
  bodyFailures: number[];
  /// 按 session 记录的未定型占位，渲染在 transcript 末尾。
  placeholders: Record<string, MessagePlaceholder[]>;
  /// 已应用的最大事实事件 id；`null` 表示尚未装配快照基线。
  lastEventId: number | null;
  applied: AppliedFact[];
};

export const emptyNodeState: NodeState = {
  modelName: '',
  workspaces: [],
  sessions: [],
  trees: {},
  retries: {},
  bodies: {},
  bodyRequests: [],
  bodyFailures: [],
  placeholders: {},
  lastEventId: null,
  applied: [],
};

/// 应用快照后的水位。尚无基线时取快照水位；已有基线时回退到较老水位，
/// 使快照水位之后已应用的事实被重放。
export function snapshotCursor(cursor: number | null, watermark: number): number {
  return cursor === null ? watermark : Math.min(cursor, watermark);
}

export function loadNodeBootstrap(state: NodeState, bootstrap: BootstrapResponse): NodeState {
  return rebase(state, bootstrap.maxEventId, {
    ...state,
    modelName: bootstrap.model,
    workspaces: bootstrap.workspaces,
    sessions: bootstrap.sessions,
    lastEventId: snapshotCursor(state.lastEventId, bootstrap.maxEventId),
  });
}

export function applyNodeFact(state: NodeState, id: number, event: FactEvent): NodeState {
  if (state.lastEventId !== null && id <= state.lastEventId) {
    return state;
  }
  const applied = applyFactEvent({ ...state, lastEventId: id }, event);
  return {
    ...applied,
    applied: [...state.applied.filter((fact) => fact.id !== id), { id, event }].slice(
      -APPLIED_FACT_LIMIT,
    ),
  };
}

export function applyFactEvent(state: NodeState, event: FactEvent): NodeState {
  switch (event.kind) {
    case 'workspace_registered':
      return { ...state, workspaces: upsertWorkspace(state.workspaces, event.workspace) };
    case 'workspace_removed':
      return removeWorkspace(state, event.path);
    case 'session_created':
    case 'session_renamed':
      return { ...state, sessions: upsertSession(state.sessions, event.session) };
    case 'message_appended':
      return dropPlaceholder(
        withSession(state, event.session, (tree) => ({
          ...tree,
          nodes: upsertNode(tree.nodes, event.message),
        })),
        event.session.id,
        event.message.id,
      );
    case 'message_updated': {
      const updated = withSession(state, event.session, (tree) => ({
        ...tree,
        nodes: upsertNode(tree.nodes, event.message),
      }));
      return dropPlaceholder(
        invalidateBody(updated, event.message.id),
        event.session.id,
        event.message.id,
      );
    }
    case 'active_leaf_changed':
      return withSession(state, event.session, (tree) => ({
        ...tree,
        activeLeaf: event.activeLeaf,
      }));
    case 'message_queued':
      return withSession(state, event.session, (tree) => ({
        ...tree,
        queue: appendQueue(tree.queue, event.item),
      }));
    case 'run_started':
      return clearRetry(
        withSession(state, event.session, (tree) => ({
          ...tree,
          runs: upsertRun(tree.runs, event.run),
        })),
        event.session.id,
      );
    case 'run_finished':
      return clearRetry(
        dropFinished(
          withSession(state, event.session, (tree) => ({
            ...tree,
            runs: upsertRun(tree.runs, event.run),
          })),
          event.session.id,
        ),
        event.session.id,
      );
    case 'queue_item_removed':
      return removeQueueItem(state, event.sessionId, event.itemId);
    case 'session_deleted':
      return removeSession(state, event.sessionId);
    default: {
      const unhandled: never = event;
      throw new Error(`未处理的事实事件: ${JSON.stringify(unhandled)}`);
    }
  }
}

/// 记录一次限流重试等待。
export function applyRetry(state: NodeState, sessionId: string, retry: RetryState): NodeState {
  return { ...state, retries: { ...state.retries, [sessionId]: retry } };
}

export function loadSessionTree(state: NodeState, tree: SessionTree): NodeState {
  const loaded = rebase(state, tree.maxEventId, {
    ...state,
    lastEventId: snapshotCursor(state.lastEventId, tree.maxEventId),
    sessions: upsertSession(state.sessions, tree.session),
    trees: {
      ...state.trees,
      [tree.session.id]: {
        activeLeaf: tree.activeLeaf,
        nodes: tree.nodes,
        queue: tree.queue,
        runs: tree.runs,
      },
    },
    bodyRequests: [],
    bodyFailures: [],
    placeholders: { ...state.placeholders, [tree.session.id]: [] },
  });
  return clearRetry(loaded, tree.session.id);
}

export function storeSession(state: NodeState, session: SessionMetadata): NodeState {
  return { ...state, sessions: upsertSession(state.sessions, session) };
}

export function sessionById(state: NodeState, id: string): SessionMetadata | undefined {
  return state.sessions.find((session) => session.id === id);
}

/// 快照替换视图后重放水位之后已应用的事实，使快照不覆盖更新的尾部。
function rebase(state: NodeState, watermark: number, base: NodeState): NodeState {
  return state.applied
    .filter((fact) => fact.id > watermark)
    .reduce((current, fact) => applyNodeFact(current, fact.id, fact.event), base);
}

function withSession(
  state: NodeState,
  session: SessionMetadata,
  update: (tree: SessionTreeState) => SessionTreeState,
): NodeState {
  const sessions = upsertSession(state.sessions, session);
  const tree = state.trees[session.id] ?? emptyTree();
  return { ...state, sessions, trees: { ...state.trees, [session.id]: update(tree) } };
}

function emptyTree(): SessionTreeState {
  return { activeLeaf: null, nodes: [], queue: [], runs: [] };
}

/// 清除某个 session 的重试等待。其它 session 不受影响。
export function clearRetry(state: NodeState, sessionId: string): NodeState {
  if (state.retries[sessionId] === undefined) {
    return state;
  }
  return { ...state, retries: withoutRetryKey(state.retries, sessionId) };
}

function removeQueueItem(state: NodeState, sessionId: string, itemId: number): NodeState {
  const sessions = state.sessions.map((session) =>
    session.id === sessionId
      ? { ...session, queueLength: Math.max(0, session.queueLength - 1) }
      : session,
  );
  const tree = state.trees[sessionId];
  if (tree === undefined) {
    return { ...state, sessions };
  }
  return {
    ...state,
    sessions,
    trees: {
      ...state.trees,
      [sessionId]: { ...tree, queue: tree.queue.filter((item) => item.id !== itemId) },
    },
  };
}

function removeSession(state: NodeState, sessionId: string): NodeState {
  const removed = forgetBodies(state, messageIds(state, sessionId));
  return {
    ...removed,
    sessions: removed.sessions.filter((session) => session.id !== sessionId),
    trees: withoutKey(removed.trees, sessionId),
    retries: withoutRetryKey(removed.retries, sessionId),
    placeholders: withoutKey(removed.placeholders, sessionId),
  };
}

/// 该 session 树里全部消息 id。
function messageIds(state: NodeState, sessionId: string): number[] {
  return (state.trees[sessionId]?.nodes ?? []).map((node) => node.id);
}

function removeWorkspace(state: NodeState, path: string): NodeState {
  const removed = state.sessions
    .filter((session) => session.workspace === path)
    .map((session) => session.id);
  let next = state;
  for (const id of removed) {
    next = removeSession(next, id);
  }
  return {
    ...next,
    workspaces: next.workspaces.filter((workspace) => workspace.path !== path),
    sessions: next.sessions.filter((session) => session.workspace !== path),
  };
}

function withoutRetryKey(
  retries: Record<string, RetryState>,
  sessionId: string,
): Record<string, RetryState> {
  if (retries[sessionId] === undefined) {
    return retries;
  }
  return Object.fromEntries(Object.entries(retries).filter(([id]) => id !== sessionId));
}

function withoutKey<T>(entries: Record<string, T>, key: string): Record<string, T> {
  return Object.fromEntries(Object.entries(entries).filter(([id]) => id !== key));
}

function upsertWorkspace(
  workspaces: WorkspaceMetadata[],
  incoming: WorkspaceMetadata,
): WorkspaceMetadata[] {
  if (workspaces.some((workspace) => workspace.path === incoming.path)) {
    return workspaces;
  }
  return [...workspaces, incoming];
}

function upsertSession(sessions: SessionMetadata[], incoming: SessionMetadata): SessionMetadata[] {
  if (!sessions.some((session) => session.id === incoming.id)) {
    return [...sessions, incoming];
  }
  return sessions.map((session) => (session.id === incoming.id ? incoming : session));
}

function upsertNode(nodes: MessageSkeleton[], incoming: MessageSkeleton): MessageSkeleton[] {
  if (!nodes.some((node) => node.id === incoming.id)) {
    return [...nodes, incoming];
  }
  return nodes.map((node) => (node.id === incoming.id ? incoming : node));
}

function appendQueue(queue: QueueItem[], incoming: QueueItem): QueueItem[] {
  if (queue.some((item) => item.id === incoming.id)) {
    return queue;
  }
  return [...queue, incoming];
}

function upsertRun(runs: RunRecord[], incoming: RunRecord): RunRecord[] {
  if (!runs.some((run) => run.id === incoming.id)) {
    return [...runs, incoming];
  }
  return runs.map((run) => (run.id === incoming.id ? incoming : run));
}
