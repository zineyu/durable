import type { BootstrapResponse } from '../api/generated/BootstrapResponse';
import type { ContentPart } from '../api/generated/ContentPart';
import type { FactEvent } from '../api/generated/FactEvent';
import type { HostSnapshot } from '../api/generated/HostSnapshot';
import type { MessageSkeleton } from '../api/generated/MessageSkeleton';
import type { QueueItem } from '../api/generated/QueueItem';
import type { RunRecord } from '../api/generated/RunRecord';
import type { SessionMetadata } from '../api/generated/SessionMetadata';
import type { SessionTreeSnapshot } from '../api/generated/SessionTreeSnapshot';
import type { WorkspaceMetadata } from '../api/generated/WorkspaceMetadata';
import { dropFinished, dropPlaceholder, invalidateBody } from './bodies';
import {
  removeSession,
  removeWorkspace,
  upsertHost,
  upsertSession,
  upsertWorkspace,
  withoutRetryKey,
} from './state-helpers';
import type {
  ClientError,
  ConnectionState,
  MessagePlaceholder,
  RetryState,
} from './types';

/// 本模块负责客户端唯一一份状态与事实归约：host 表、workspace、session、树、正文缓存与游标。
/// 动作分派在 `reducer`，派生视图在 `derived`，正文缓存的状态迁移在 `bodies`。

/// 一条事实事件的信封：事件 id、事实提交后的 session 视图与事实本体。
export type FactEnvelope = {
  id: number;
  session: SessionMetadata | null;
  event: FactEvent;
};

/// 保留的已应用事实条数。快照与已应用尾部之间的差距远小于它。
const APPLIED_FACT_LIMIT = 128;

/// 树状态。除快照信封的两个字段外全部保留，因此协议新增字段会自动流入。
export type SessionTreeState = Omit<SessionTreeSnapshot, 'session' | 'maxEventId'>;

/// 客户端全部状态。事件流只有一个游标，事实只归约到这一份状态。
/// 规格 user story R-43：只面对一条事件流与一个游标。
export type ClientState = {
  modelName: string;
  hosts: HostSnapshot[];
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
  /// 按 session 记录的最近查看时间。首次见到 session 时取当时的更新时间。
  lastSeenAt: Record<string, number>;
  /// 已应用的最大事实事件 id；`null` 表示尚未装配快照基线。
  lastEventId: number | null;
  /// 已应用的事实。快照可能比已应用的尾部旧，用它补齐快照水位之后的事实。
  applied: FactEnvelope[];
  connection: ConnectionState;
  error: ClientError | null;
};

export const emptyClientState: ClientState = {
  modelName: '',
  hosts: [],
  workspaces: [],
  sessions: [],
  trees: {},
  retries: {},
  bodies: {},
  bodyRequests: [],
  bodyFailures: [],
  placeholders: {},
  lastSeenAt: {},
  lastEventId: null,
  applied: [],
  connection: 'reconnecting',
  error: null,
};

/// 应用快照后的水位。尚无基线时取快照水位；已有基线时回退到较老水位，
/// 使快照水位之后已应用的事实被重放。
export function snapshotCursor(cursor: number | null, watermark: number): number {
  return cursor === null ? watermark : Math.min(cursor, watermark);
}

export function loadBootstrap(state: ClientState, bootstrap: BootstrapResponse): ClientState {
  return rebase(state, bootstrap.maxEventId, {
    ...state,
    modelName: bootstrap.model,
    hosts: bootstrap.hosts,
    workspaces: bootstrap.workspaces,
    sessions: bootstrap.sessions,
    lastSeenAt: bootstrap.sessions.reduce(seedSeenAt, state.lastSeenAt),
    lastEventId: snapshotCursor(state.lastEventId, bootstrap.maxEventId),
  });
}

export function applyFact(state: ClientState, fact: FactEnvelope): ClientState {
  if (state.lastEventId !== null && fact.id <= state.lastEventId) {
    return state;
  }
  const applied = applyFactEvent({ ...state, lastEventId: fact.id }, fact);
  return {
    ...applied,
    applied: [...state.applied.filter((entry) => entry.id !== fact.id), fact].slice(
      -APPLIED_FACT_LIMIT,
    ),
  };
}

/// 属于 session 的事实的信封视图。协议保证这类事实一定带视图。
function sessionOf(fact: FactEnvelope): SessionMetadata {
  const session = fact.session;
  if (session === null) {
    throw new Error(`事实事件 ${fact.id} 缺少 session 视图`);
  }
  return session;
}

export function applyFactEvent(state: ClientState, fact: FactEnvelope): ClientState {
  const event = fact.event;
  switch (event.kind) {
    case 'host_registered':
    case 'host_updated':
      return { ...state, hosts: upsertHost(state.hosts, event.host) };
    case 'host_removed':
      return { ...state, hosts: state.hosts.filter((host) => host.name !== event.host) };
    case 'host_status_changed':
      return {
        ...state,
        hosts: state.hosts.map((host) =>
          host.name === event.host
            ? { ...host, status: event.status, detail: event.detail }
            : host,
        ),
      };
    case 'workspace_registered':
      return { ...state, workspaces: upsertWorkspace(state.workspaces, event.workspace) };
    case 'workspace_removed':
      return removeWorkspace(state, event.host, event.path);
    case 'session_created':
    case 'session_renamed':
      return storeSession(state, sessionOf(fact));
    case 'message_appended': {
      const session = sessionOf(fact);
      return dropPlaceholder(
        withSession(state, session, (tree) => ({
          ...tree,
          nodes: upsertMessage(tree.nodes, event.message),
        })),
        session.id,
        event.message.id,
      );
    }
    case 'message_updated': {
      const session = sessionOf(fact);
      const updated = withSession(state, session, (tree) => ({
        ...tree,
        nodes: upsertMessage(tree.nodes, event.message),
      }));
      return dropPlaceholder(
        invalidateBody(updated, event.message.id),
        session.id,
        event.message.id,
      );
    }
    case 'active_leaf_changed':
      return withSession(state, sessionOf(fact), (tree) => ({
        ...tree,
        activeLeaf: event.activeLeaf,
      }));
    case 'message_queued':
      return withSession(state, sessionOf(fact), (tree) => ({
        ...tree,
        queue: appendQueue(tree.queue, event.item),
      }));
    case 'steer_applied':
      return storeSession(state, sessionOf(fact));
    case 'steer_downgraded':
      return withSession(state, sessionOf(fact), (tree) => ({
        ...tree,
        queue: tree.queue.map((item) => (item.id === event.item.id ? event.item : item)),
      }));
    case 'run_started': {
      const session = sessionOf(fact);
      return clearRetry(
        withSession(state, session, (tree) => ({
          ...tree,
          runs: upsertRun(tree.runs, event.run),
        })),
        session.id,
      );
    }
    case 'run_finished': {
      const session = sessionOf(fact);
      return clearRetry(
        dropFinished(
          withSession(state, session, (tree) => ({
            ...tree,
            runs: upsertRun(tree.runs, event.run),
          })),
          session.id,
        ),
        session.id,
      );
    }
    case 'queue_item_removed': {
      const update = (tree: SessionTreeState): SessionTreeState => ({
        ...tree,
        queue: tree.queue.filter((item) => item.id !== event.itemId),
      });
      const session = fact.session;
      // 迁移前的 queue_item_removed 没有 session 视图：只删队列项，不改 session 元数据。
      if (session === null) {
        const tree = state.trees[event.sessionId];
        if (tree === undefined) {
          return state;
        }
        return {
          ...state,
          trees: { ...state.trees, [event.sessionId]: update(tree) },
        };
      }
      return withSession(state, session, update);
    }
    case 'session_deleted':
      return removeSession(state, event.sessionId);
    default: {
      const unhandled: never = event;
      throw new Error(`未处理的事实事件: ${JSON.stringify(unhandled)}`);
    }
  }
}

/// 记录一次限流重试等待。
export function applyRetry(
  state: ClientState,
  sessionId: string,
  retry: RetryState,
): ClientState {
  return { ...state, retries: { ...state.retries, [sessionId]: retry } };
}

export function loadSessionTree(state: ClientState, tree: SessionTreeSnapshot): ClientState {
  const loaded = rebase(state, tree.maxEventId, {
    ...state,
    lastEventId: snapshotCursor(state.lastEventId, tree.maxEventId),
    sessions: upsertSession(state.sessions, tree.session),
    lastSeenAt: seedSeenAt(state.lastSeenAt, tree.session),
    trees: {
      ...state.trees,
      [tree.session.id]: treeState(tree),
    },
    bodyRequests: [],
    bodyFailures: [],
    placeholders: { ...state.placeholders, [tree.session.id]: [] },
  });
  return clearRetry(loaded, tree.session.id);
}

export function storeSession(state: ClientState, session: SessionMetadata): ClientState {
  return {
    ...state,
    sessions: upsertSession(state.sessions, session),
    lastSeenAt: seedSeenAt(state.lastSeenAt, session),
  };
}

/// 记录某个 session 已被查看。时间只前进，不回退。
export function markSeen(state: ClientState, sessionId: string, at: number): ClientState {
  const seen = state.lastSeenAt[sessionId];
  if (seen !== undefined && seen >= at) {
    return state;
  }
  return { ...state, lastSeenAt: { ...state.lastSeenAt, [sessionId]: at } };
}

/// 首次见到一个 session 时把它的更新时间记为已查看，避免首屏把全部 session 标成未读。
function seedSeenAt(
  lastSeenAt: Record<string, number>,
  session: SessionMetadata,
): Record<string, number> {
  if (lastSeenAt[session.id] !== undefined) {
    return lastSeenAt;
  }
  return { ...lastSeenAt, [session.id]: session.updatedAt };
}

export function sessionById(state: ClientState, id: string): SessionMetadata | undefined {
  return state.sessions.find((session) => session.id === id);
}

/// 快照替换视图后重放水位之后已应用的事实，使快照不覆盖更新的尾部。
function rebase(state: ClientState, watermark: number, base: ClientState): ClientState {
  return state.applied
    .filter((fact) => fact.id > watermark)
    .reduce((current, fact) => applyFact(current, fact), base);
}

function withSession(
  state: ClientState,
  session: SessionMetadata,
  update: (tree: SessionTreeState) => SessionTreeState,
): ClientState {
  const sessions = upsertSession(state.sessions, session);
  const tree = state.trees[session.id] ?? emptyTree();
  return {
    ...state,
    sessions,
    lastSeenAt: seedSeenAt(state.lastSeenAt, session),
    trees: { ...state.trees, [session.id]: update(tree) },
  };
}

function emptyTree(): SessionTreeState {
  return { activeLeaf: null, nodes: [], queue: [], runs: [] };
}

/// 去掉快照信封的 session 视图与水位；其余字段整体进入状态。
function treeState(tree: SessionTreeSnapshot): SessionTreeState {
  const { session, maxEventId, ...rest } = tree;
  return rest;
}

/// 清除某个 session 的重试等待。其它 session 不受影响。
export function clearRetry(state: ClientState, sessionId: string): ClientState {
  if (state.retries[sessionId] === undefined) {
    return state;
  }
  return { ...state, retries: withoutRetryKey(state.retries, sessionId) };
}

function upsertMessage(
  nodes: MessageSkeleton[],
  incoming: MessageSkeleton,
): MessageSkeleton[] {
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
