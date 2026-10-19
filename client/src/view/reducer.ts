import type { BootstrapResponse } from '../api/generated/BootstrapResponse';
import type { FactEvent } from '../api/generated/FactEvent';
import type { MessageNode } from '../api/generated/MessageNode';
import type { QueueItem } from '../api/generated/QueueItem';
import type { RunRecord } from '../api/generated/RunRecord';
import type { SessionMetadata } from '../api/generated/SessionMetadata';
import type { SessionTree } from '../api/generated/SessionTree';
import type { StreamEvent } from '../api/generated/StreamEvent';
import type { ToolCallRecord } from '../api/generated/ToolCallRecord';
import type { WorkspaceMetadata } from '../api/generated/WorkspaceMetadata';
import { toSessionId, toWorkspaceId, toWorkspacePath } from './ids';
import { runFailureLabel } from './labels';
import type { ConnectionState, MessageState, SessionSummary, WorkspaceSummary } from './types';

export type SessionTreeState = {
  activeLeaf: number | null;
  nodes: MessageNode[];
  queue: QueueItem[];
  runs: RunRecord[];
};

export type ClientError = {
  code: string;
  message: string;
};

export type ClientState = {
  modelName: string;
  workspaces: WorkspaceMetadata[];
  sessions: SessionMetadata[];
  trees: Record<string, SessionTreeState>;
  connection: ConnectionState;
  /// 已应用的最大事实事件 id；`null` 表示尚未装配快照基线。
  lastEventId: number | null;
  error: ClientError | null;
};

export type ClientAction =
  | { type: 'bootstrap'; bootstrap: BootstrapResponse }
  | { type: 'stream'; event: StreamEvent }
  | { type: 'tree'; tree: SessionTree }
  | { type: 'session'; session: SessionMetadata }
  | { type: 'connection'; connection: ConnectionState }
  | { type: 'error'; error: ClientError }
  | { type: 'dismiss-error' };

export const initialClientState: ClientState = {
  modelName: '',
  workspaces: [],
  sessions: [],
  trees: {},
  connection: 'reconnecting',
  lastEventId: null,
  error: null,
};

/// 应用快照后的游标。尚无基线时取快照水位；已有基线时回退到较老水位，
/// 使事件流用 `after` 重放快照未覆盖的尾部。
export function snapshotCursor(cursor: number | null, watermark: number): number {
  return cursor === null ? watermark : Math.min(cursor, watermark);
}

export function reduce(state: ClientState, action: ClientAction): ClientState {
  switch (action.type) {
    case 'bootstrap':
      return {
        ...state,
        modelName: action.bootstrap.model,
        workspaces: action.bootstrap.workspaces,
        sessions: action.bootstrap.sessions,
        lastEventId: snapshotCursor(state.lastEventId, action.bootstrap.maxEventId),
      };
    case 'stream':
      return applyStreamEvent(state, action.event);
    case 'tree':
      return loadSessionTree(state, action.tree);
    case 'session':
      return { ...state, sessions: upsertSession(state.sessions, action.session) };
    case 'connection':
      return { ...state, connection: action.connection };
    case 'error':
      return { ...state, error: action.error };
    case 'dismiss-error':
      return { ...state, error: null };
    default: {
      const unhandled: never = action;
      throw new Error(`未处理的动作: ${JSON.stringify(unhandled)}`);
    }
  }
}

export function applyStreamEvent(state: ClientState, event: StreamEvent): ClientState {
  switch (event.type) {
    case 'fact': {
      if (state.lastEventId !== null && event.id <= state.lastEventId) {
        return state;
      }
      return applyFactEvent({ ...state, lastEventId: event.id }, event.event);
    }
    case 'delta':
      return applyDelta(state, event.sessionId, event.messageId, event.text);
    case 'resync':
      return state;
    default: {
      const unhandled: never = event;
      throw new Error(`未处理的事件: ${JSON.stringify(unhandled)}`);
    }
  }
}

export function applyFactEvent(state: ClientState, event: FactEvent): ClientState {
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
      return withSession(state, event.session, (tree) => ({
        ...tree,
        runs: upsertRun(tree.runs, event.run),
      }));
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
  state: ClientState,
  sessionId: string,
  messageId: number,
  text: string,
): ClientState {
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
  return { ...state, trees: { ...state.trees, [sessionId]: { ...tree, nodes } } };
}

export function loadSessionTree(state: ClientState, tree: SessionTree): ClientState {
  return {
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
  };
}

export function sessionById(state: ClientState, id: string): SessionMetadata | undefined {
  return state.sessions.find((session) => session.id === id);
}

export function transcriptNodes(state: ClientState, sessionId: string): MessageNode[] {
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

export function sessionQueue(state: ClientState, sessionId: string): QueueItem[] {
  return state.trees[sessionId]?.queue ?? [];
}

export function messageView(
  state: ClientState,
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
    case 'running':
      return { state: 'streaming', note: null };
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
  state: ClientState,
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

export function sidebarWorkspaces(state: ClientState): WorkspaceSummary[] {
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

function withSession(
  state: ClientState,
  session: SessionMetadata,
  update: (tree: SessionTreeState) => SessionTreeState,
): ClientState {
  const sessions = upsertSession(state.sessions, session);
  const tree = state.trees[session.id] ?? emptyTree();
  return { ...state, sessions, trees: { ...state.trees, [session.id]: update(tree) } };
}

function emptyTree(): SessionTreeState {
  return { activeLeaf: null, nodes: [], queue: [], runs: [] };
}

function removeQueueItem(state: ClientState, sessionId: string, itemId: number): ClientState {
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

function removeSession(state: ClientState, sessionId: string): ClientState {
  return {
    ...state,
    sessions: state.sessions.filter((session) => session.id !== sessionId),
    trees: withoutKey(state.trees, sessionId),
  };
}

function removeWorkspace(state: ClientState, path: string): ClientState {
  const removed = new Set(
    state.sessions.filter((session) => session.workspace === path).map((session) => session.id),
  );
  let trees = state.trees;
  for (const id of removed) {
    trees = withoutKey(trees, id);
  }
  return {
    ...state,
    workspaces: state.workspaces.filter((workspace) => workspace.path !== path),
    sessions: state.sessions.filter((session) => session.workspace !== path),
    trees,
  };
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
