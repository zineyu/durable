import type { BootstrapResponse } from '../api/generated/BootstrapResponse';
import type { FactEvent } from '../api/generated/FactEvent';
import type { MessageNode } from '../api/generated/MessageNode';
import type { QueueItem } from '../api/generated/QueueItem';
import type { RunRecord } from '../api/generated/RunRecord';
import type { SessionMetadata } from '../api/generated/SessionMetadata';
import type { SessionTree } from '../api/generated/SessionTree';
import type { ToolCallRecord } from '../api/generated/ToolCallRecord';
import type { WorkspaceMetadata } from '../api/generated/WorkspaceMetadata';
import { toSessionId, toWorkspaceId, toWorkspacePath } from './ids';
import { retryNote, runFailureLabel } from './labels';
import type { MessageState, RetryState, SessionSummary, WorkspaceSummary } from './types';

/// 已应用的事实。快照可能比已应用的尾部旧，用它补齐快照水位之后的事实。
type AppliedFact = {
  id: number;
  event: FactEvent;
};

/// 每节点保留的已应用事实条数。快照与已应用尾部之间的差距远小于它。
const APPLIED_FACT_LIMIT = 128;

export type SessionTreeState = {
  activeLeaf: number | null;
  nodes: MessageNode[];
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
    case 'message_updated':
      return withSession(state, event.session, (tree) => ({
        ...tree,
        nodes: upsertNode(tree.nodes, event.message),
      }));
    case 'tool_call_recorded':
      return withSession(state, event.session, (tree) => ({
        ...tree,
        nodes: appendToolCall(tree.nodes, event.message, event.toolCall),
      }));
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
    case 'run_finished':
      return clearRetry(
        withSession(state, event.session, (tree) => ({
          ...tree,
          runs: upsertRun(tree.runs, event.run),
        })),
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

export function applyDelta(
  state: NodeState,
  sessionId: string,
  messageId: number,
  text: string,
): NodeState {
  const tree = state.trees[sessionId];
  if (tree === undefined) {
    return state;
  }
  const index = tree.nodes.findIndex((node) => node.id === messageId);
  if (index === -1) {
    return state;
  }
  const nodes = tree.nodes.map((node, position) =>
    position === index ? { ...node, text: node.text + text } : node,
  );
  return clearRetry(
    { ...state, trees: { ...state.trees, [sessionId]: { ...tree, nodes } } },
    sessionId,
  );
}

/// 记录一次限流重试等待。
export function applyRetry(state: NodeState, sessionId: string, retry: RetryState): NodeState {
  return { ...state, retries: { ...state.retries, [sessionId]: retry } };
}

/// 清除某个 session 的重试等待。其它 session 不受影响。
function clearRetry(state: NodeState, sessionId: string): NodeState {
  if (state.retries[sessionId] === undefined) {
    return state;
  }
  return { ...state, retries: withoutRetryKey(state.retries, sessionId) };
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
  });
  return clearRetry(loaded, tree.session.id);
}

export function storeSession(state: NodeState, session: SessionMetadata): NodeState {
  return { ...state, sessions: upsertSession(state.sessions, session) };
}

export function sessionById(state: NodeState, id: string): SessionMetadata | undefined {
  return state.sessions.find((session) => session.id === id);
}

export function transcriptNodes(state: NodeState, sessionId: string): MessageNode[] {
  const tree = state.trees[sessionId];
  if (tree === undefined || tree.activeLeaf === null) {
    return [];
  }
  const byId = new Map(tree.nodes.map((node) => [node.id, node]));
  const path: MessageNode[] = [];
  let current = byId.get(tree.activeLeaf);
  while (current !== undefined) {
    path.push(current);
    if (current.parentId === null) {
      break;
    }
    current = byId.get(current.parentId);
  }
  return path.reverse();
}

export function sessionQueue(state: NodeState, sessionId: string): QueueItem[] {
  return state.trees[sessionId]?.queue ?? [];
}

export function messageView(
  state: NodeState,
  sessionId: string,
  node: MessageNode,
): { state: MessageState; note: string | null } {
  if (node.runId === null) {
    return { state: 'complete', note: null };
  }
  const run = state.trees[sessionId]?.runs.find((candidate) => candidate.id === node.runId);
  if (run === undefined) {
    return { state: 'complete', note: null };
  }
  switch (run.status) {
    case 'running': {
      const retry = state.retries[sessionId];
      return { state: 'streaming', note: retry === undefined ? null : retryNote(retry) };
    }
    case 'failed':
      return {
        state: 'failed',
        note:
          run.failure === null
            ? null
            : `${runFailureLabel[run.failure.code]}：${run.failure.message}`,
      };
    case 'cancelled':
      return { state: 'cancelled', note: null };
    case 'succeeded':
      return { state: 'complete', note: null };
    default: {
      const unhandled: never = run.status;
      throw new Error(`未处理的 run 状态: ${JSON.stringify(unhandled)}`);
    }
  }
}

export type BranchPosition = {
  index: number;
  total: number;
  previousId: number | null;
  nextId: number | null;
};

export function branchPosition(
  state: NodeState,
  sessionId: string,
  node: MessageNode,
): BranchPosition | null {
  const nodes = state.trees[sessionId]?.nodes ?? [];
  const siblings = nodes
    .filter((candidate) => candidate.parentId === node.parentId)
    .sort((left, right) => left.id - right.id);
  if (siblings.length < 2) {
    return null;
  }
  const index = siblings.findIndex((candidate) => candidate.id === node.id);
  return {
    index: index + 1,
    total: siblings.length,
    previousId: siblings[index - 1]?.id ?? null,
    nextId: siblings[index + 1]?.id ?? null,
  };
}

export function sidebarWorkspaces(state: NodeState): WorkspaceSummary[] {
  const workspaces = [...state.workspaces].sort(compareWorkspaces);
  return workspaces.map((workspace) => ({
    id: toWorkspaceId(workspace.path),
    path: toWorkspacePath(workspace.path),
    sessions: state.sessions
      .filter((session) => session.workspace === workspace.path)
      .sort(compareSessions)
      .map(toSessionSummary),
  }));
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
  return {
    ...state,
    sessions: state.sessions.filter((session) => session.id !== sessionId),
    trees: withoutKey(state.trees, sessionId),
    retries: withoutRetryKey(state.retries, sessionId),
  };
}

function removeWorkspace(state: NodeState, path: string): NodeState {
  const removed = new Set(
    state.sessions.filter((session) => session.workspace === path).map((session) => session.id),
  );
  let trees = state.trees;
  let retries = state.retries;
  for (const id of removed) {
    trees = withoutKey(trees, id);
    retries = withoutRetryKey(retries, id);
  }
  return {
    ...state,
    workspaces: state.workspaces.filter((workspace) => workspace.path !== path),
    sessions: state.sessions.filter((session) => session.workspace !== path),
    trees,
    retries,
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

function withoutKey(
  trees: Record<string, SessionTreeState>,
  key: string,
): Record<string, SessionTreeState> {
  return Object.fromEntries(Object.entries(trees).filter(([id]) => id !== key));
}

function compareWorkspaces(left: WorkspaceMetadata, right: WorkspaceMetadata): number {
  return left.registeredAt - right.registeredAt || left.path.localeCompare(right.path);
}

function compareSessions(left: SessionMetadata, right: SessionMetadata): number {
  return right.updatedAt - left.updatedAt;
}

function toSessionSummary(session: SessionMetadata): SessionSummary {
  const state = session.busy ? 'running' : session.queueLength > 0 ? 'queued' : 'idle';
  return {
    id: toSessionId(session.id),
    title: session.title,
    state,
    queuedCount: session.queueLength,
    updatedAt: session.updatedAt,
  };
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

function upsertNode(nodes: MessageNode[], incoming: MessageNode): MessageNode[] {
  if (!nodes.some((node) => node.id === incoming.id)) {
    return [...nodes, incoming];
  }
  return nodes.map((node) => (node.id === incoming.id ? incoming : node));
}

/// 追加已完成的工具调用；同一调用重复到达时保持原状态。
function appendToolCall(
  nodes: MessageNode[],
  messageId: number,
  record: ToolCallRecord,
): MessageNode[] {
  return nodes.map((node) =>
    node.id === messageId && !node.toolCalls.some((call) => call.id === record.id)
      ? { ...node, toolCalls: [...node.toolCalls, record] }
      : node,
  );
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
