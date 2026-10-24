import type { BootstrapResponse } from '../api/generated/BootstrapResponse';
import type { HubBootstrap } from '../api/generated/HubBootstrap';
import type { HubStreamEvent } from '../api/generated/HubStreamEvent';
import type { MessageBody } from '../api/generated/MessageBody';
import type { NodeName } from '../api/generated/NodeName';
import type { SessionMetadata } from '../api/generated/SessionMetadata';
import type { SessionTree } from '../api/generated/SessionTree';
import {
  applyDelta,
  applyMessageBodies,
  failMessageBodies,
  requestMessageBodies,
  retryMessageBodies,
} from './bodies';
import {
  applyNodeFact,
  applyRetry,
  clearRetry,
  emptyNodeState,
  loadNodeBootstrap,
  loadSessionTree,
  markSeen,
  snapshotCursor,
  storeSession,
} from './node-state';
import type { NodeState } from './node-state';
import type { ConnectionState, NodeRecord } from './types';

export type HubState = {
  nodes: NodeRecord[];
  selected: NodeName | null;
  /// 已应用的最大广播序号；`null` 表示尚未装配合并 bootstrap。
  cursor: number | null;
};

export type ClientError = {
  code: string;
  message: string;
};

export type ClientState = {
  hub: HubState;
  /// 每节点的分片。非 `ok` 节点没有分片，其列表为空。
  nodes: Record<NodeName, NodeState>;
  connection: ConnectionState;
  error: ClientError | null;
};

export type ClientAction =
  | { type: 'bootstrap'; bootstrap: HubBootstrap }
  | { type: 'node-bootstrap'; node: NodeName; bootstrap: BootstrapResponse }
  | { type: 'stream'; event: HubStreamEvent }
  | { type: 'tree'; node: NodeName; tree: SessionTree }
  | { type: 'session'; node: NodeName; session: SessionMetadata }
  | { type: 'bodies-request'; node: NodeName; ids: number[] }
  | { type: 'bodies-failed'; node: NodeName; ids: number[] }
  | { type: 'bodies-retry'; node: NodeName; ids: number[] }
  | { type: 'bodies'; node: NodeName; ids: number[]; messages: MessageBody[] }
  | { type: 'select-node'; node: NodeName | null }
  | { type: 'seen'; node: NodeName; sessionId: string; at: number }
  | { type: 'connection'; connection: ConnectionState }
  | { type: 'error'; error: ClientError }
  | { type: 'dismiss-error' };

export const initialClientState: ClientState = {
  hub: { nodes: [], selected: null, cursor: null },
  nodes: {},
  connection: 'reconnecting',
  error: null,
};

export function reduce(state: ClientState, action: ClientAction): ClientState {
  switch (action.type) {
    case 'bootstrap':
      return applyBootstrap(state, action.bootstrap);
    case 'node-bootstrap':
      return withShard(
        state,
        action.node,
        loadNodeBootstrap(state.nodes[action.node] ?? emptyNodeState, action.bootstrap),
      );
    case 'stream':
      return applyStreamEvent(state, action.event);
    case 'tree':
      return withShard(
        state,
        action.node,
        loadSessionTree(state.nodes[action.node] ?? emptyNodeState, action.tree),
      );
    case 'session':
      return withShard(
        state,
        action.node,
        storeSession(state.nodes[action.node] ?? emptyNodeState, action.session),
      );
    case 'bodies-request':
      return mapShard(state, action.node, (shard) => requestMessageBodies(shard, action.ids));
    case 'bodies-failed':
      return mapShard(state, action.node, (shard) => failMessageBodies(shard, action.ids));
    case 'bodies-retry':
      return mapShard(state, action.node, (shard) => retryMessageBodies(shard, action.ids));
    case 'bodies':
      return mapShard(state, action.node, (shard) =>
        applyMessageBodies(shard, action.ids, action.messages),
      );
    case 'select-node':
      return { ...state, hub: { ...state.hub, selected: action.node } };
    case 'seen':
      return mapShard(state, action.node, (shard) => markSeen(shard, action.sessionId, action.at));
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

function applyBootstrap(state: ClientState, bootstrap: HubBootstrap): ClientState {
  const nodes: Record<NodeName, NodeState> = {};
  for (const snapshot of bootstrap.nodes) {
    if (snapshot.bootstrap !== null) {
      nodes[snapshot.name] = loadNodeBootstrap(
        state.nodes[snapshot.name] ?? emptyNodeState,
        snapshot.bootstrap,
      );
    }
  }
  return {
    ...state,
    hub: {
      nodes: bootstrap.nodes.map((snapshot) => ({
        name: snapshot.name,
        url: snapshot.url,
        status: snapshot.status,
        detail: snapshot.detail,
      })),
      selected: state.hub.selected,
      cursor: snapshotCursor(state.hub.cursor, bootstrap.maxHubSeq),
    },
    nodes,
  };
}

export function applyStreamEvent(state: ClientState, event: HubStreamEvent): ClientState {
  switch (event.type) {
    case 'fact': {
      const advanced = advanceCursor(state, event.hubSeq);
      if (advanced === null) {
        return state;
      }
      const shard = advanced.nodes[event.node];
      if (shard === undefined) {
        return advanced;
      }
      return withShard(advanced, event.node, applyNodeFact(shard, event.id, event.event));
    }
    case 'delta': {
      const shard = state.nodes[event.node];
      if (shard === undefined) {
        return state;
      }
      const next = clearRetry(
        applyDelta(shard, event.sessionId, event.messageId, event.kind, event.text),
        event.sessionId,
      );
      return next === shard ? state : withShard(state, event.node, next);
    }
    case 'retry': {
      const shard = state.nodes[event.node];
      if (shard === undefined) {
        return state;
      }
      return withShard(
        state,
        event.node,
        applyRetry(shard, event.sessionId, {
          attempt: event.attempt,
          maxAttempts: event.maxAttempts,
          delayMs: event.delayMs,
        }),
      );
    }
    case 'node_registered': {
      const advanced = advanceCursor(state, event.hubSeq);
      if (advanced === null) {
        return state;
      }
      return withNodeRecords(
        advanced,
        upsertNodeRecord(advanced.hub.nodes, {
          name: event.node.name,
          url: event.node.url,
          status: 'unreachable',
          detail: null,
        }),
      );
    }
    case 'node_removed': {
      const advanced = advanceCursor(state, event.hubSeq);
      if (advanced === null) {
        return state;
      }
      return {
        ...withNodeRecords(
          advanced,
          advanced.hub.nodes.filter((record) => record.name !== event.name),
        ),
        nodes: withoutNode(advanced.nodes, event.name),
      };
    }
    case 'node_status_changed': {
      const advanced = advanceCursor(state, event.hubSeq);
      if (advanced === null) {
        return state;
      }
      const records = advanced.hub.nodes.map((record) =>
        record.name === event.name
          ? { ...record, status: event.status, detail: event.detail }
          : record,
      );
      const shards = advanced.nodes;
      return {
        ...withNodeRecords(advanced, records),
        nodes: event.status === 'ok' ? shards : withoutNode(shards, event.name),
      };
    }
    case 'resync':
      return state;
    default: {
      const unhandled: never = event;
      throw new Error(`未处理的事件: ${JSON.stringify(unhandled)}`);
    }
  }
}

/// 推进广播游标。已经应用过的序号返回 `null`，调用方保持原状态。
function advanceCursor(state: ClientState, hubSeq: number): ClientState | null {
  if (state.hub.cursor !== null && hubSeq <= state.hub.cursor) {
    return null;
  }
  return { ...state, hub: { ...state.hub, cursor: hubSeq } };
}

function withShard(state: ClientState, node: NodeName, shard: NodeState): ClientState {
  return { ...state, nodes: { ...state.nodes, [node]: shard } };
}

/// 只作用于已有分片；分片不存在时保持原状态。
function mapShard(
  state: ClientState,
  node: NodeName,
  update: (shard: NodeState) => NodeState,
): ClientState {
  const shard = state.nodes[node];
  if (shard === undefined) {
    return state;
  }
  const next = update(shard);
  return next === shard ? state : withShard(state, node, next);
}

function withNodeRecords(state: ClientState, nodes: NodeRecord[]): ClientState {
  return { ...state, hub: { ...state.hub, nodes } };
}

function upsertNodeRecord(nodes: NodeRecord[], incoming: NodeRecord): NodeRecord[] {
  if (!nodes.some((record) => record.name === incoming.name)) {
    return [...nodes, incoming];
  }
  return nodes.map((record) => (record.name === incoming.name ? incoming : record));
}

function withoutNode(
  nodes: Record<NodeName, NodeState>,
  name: NodeName,
): Record<NodeName, NodeState> {
  return Object.fromEntries(Object.entries(nodes).filter(([key]) => key !== name));
}
