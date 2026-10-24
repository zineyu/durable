import { describe, expect, it } from 'vitest';

import type { BootstrapResponse } from '../api/generated/BootstrapResponse';
import type { ContentPart } from '../api/generated/ContentPart';
import type { FactEvent } from '../api/generated/FactEvent';
import type { HubBootstrap } from '../api/generated/HubBootstrap';
import type { HubStreamEvent } from '../api/generated/HubStreamEvent';
import type { MessageSkeleton } from '../api/generated/MessageSkeleton';
import type { NodeSnapshot } from '../api/generated/NodeSnapshot';
import type { NodeStatus } from '../api/generated/NodeStatus';
import type { PartSkeleton } from '../api/generated/PartSkeleton';
import type { RunRecord } from '../api/generated/RunRecord';
import type { SessionMetadata } from '../api/generated/SessionMetadata';
import type { SessionTree } from '../api/generated/SessionTree';
import type { WorkspaceMetadata } from '../api/generated/WorkspaceMetadata';
import { toSessionId } from './ids';
import { bodyOf, missingBodyIds, placeholdersOf, toolResults } from './bodies';
import {
  branchPosition,
  messageView,
  sessionQueue,
  transcriptNodes,
} from './messages';
import { loadSessionTree, snapshotCursor } from './node-state';
import { sidebarWorkspaces } from './sidebar';
import type { NodeState } from './node-state';
import { initialClientState, reduce } from './reducer';
import type { ClientState } from './reducer';

const nodeA = 'macbook-air-01';
const nodeB = 'studio';

const workspaceA: WorkspaceMetadata = { path: '/work/a', registeredAt: 10 };
const workspaceB: WorkspaceMetadata = { path: '/work/b', registeredAt: 20 };

function session(
  id: string,
  workspace: string,
  updatedAt: number,
  extra: Partial<SessionMetadata> = {},
): SessionMetadata {
  return {
    id,
    workspace,
    title: `标题 ${id}`,
    busy: false,
    queueLength: 0,
    updatedAt,
    ...extra,
  };
}

function message(
  id: number,
  parentId: number | null,
  role: MessageSkeleton['role'],
  extra: Partial<MessageSkeleton> = {},
): MessageSkeleton {
  return {
    id,
    parentId,
    role,
    runId: null,
    createdAt: id,
    parts: [],
    ...extra,
  };
}

function parts(kinds: PartSkeleton['kind'][]): PartSkeleton[] {
  return kinds.map((kind, index) => ({ seq: index, kind, size: 1, state: 'complete' }));
}

function text(value: string): ContentPart[] {
  return [{ kind: 'text', text: value, providerMeta: null }];
}

function bootstrap(
  workspaces: WorkspaceMetadata[],
  sessions: SessionMetadata[],
  maxEventId = 7,
): BootstrapResponse {
  return { model: 'gpt-5-codex', maxEventId, workspaces, sessions };
}

function snapshot(
  name: string,
  node: BootstrapResponse | null,
  status: NodeStatus = 'ok',
  detail: string | null = null,
): NodeSnapshot {
  return { name, url: `http://127.0.0.1:${name}`, status, detail, bootstrap: node };
}

function hub(nodes: NodeSnapshot[], maxHubSeq: number): HubBootstrap {
  return { nodes, maxHubSeq };
}

function loaded(): ClientState {
  return reduce(initialClientState, {
    type: 'bootstrap',
    bootstrap: hub(
      [
        snapshot(nodeA, bootstrap([workspaceA], [session('s-1', '/work/a', 100)])),
        snapshot(nodeB, bootstrap([workspaceB], [session('s-1', '/work/b', 300)])),
      ],
      7,
    ),
  });
}

function fact(node: string, hubSeq: number, id: number, event: FactEvent): HubStreamEvent {
  return { type: 'fact', hubSeq, node, id, sessionId: null, timestamp: id, event };
}

function streamed(state: ClientState, event: HubStreamEvent): ClientState {
  return reduce(state, { type: 'stream', event });
}

function delta(
  node: string,
  sessionId: string,
  messageId: number,
  value: string,
  kind: 'text' | 'reasoning' = 'text',
): HubStreamEvent {
  return {
    type: 'delta',
    node,
    sessionId: toSessionId(sessionId),
    messageId,
    kind,
    text: value,
    timestamp: 1,
  };
}

function at<T>(entries: T[], index: number): T {
  const entry = entries[index];
  if (entry === undefined) {
    throw new Error('缺少测试节点');
  }
  return entry;
}

function shardOf(state: ClientState, node: string): NodeState {
  const shard = state.nodes[node];
  if (shard === undefined) {
    throw new Error(`缺少分片: ${node}`);
  }
  return shard;
}

function tree(
  id: string,
  maxEventId: number,
  activeLeaf: number | null,
  nodes: MessageSkeleton[],
): SessionTree {
  return {
    maxEventId,
    session: session(id, '/work/a', maxEventId),
    activeLeaf,
    nodes,
    queue: [],
    runs: [],
  };
}

function withTree(state: ClientState, node: string, value: SessionTree): ClientState {
  return reduce(state, { type: 'tree', node, tree: value });
}

describe('合并 bootstrap', () => {
  it('按节点建立分片并记录广播游标', () => {
    const state = loaded();

    expect(state.hub.nodes.map((record) => record.name)).toEqual([nodeA, nodeB]);
    expect(state.hub.cursor).toBe(7);
    expect(state.hub.selected).toBeNull();
    expect(shardOf(state, nodeA).modelName).toBe('gpt-5-codex');
    expect(shardOf(state, nodeA).lastEventId).toBe(7);
    expect(sidebarWorkspaces(shardOf(state, nodeB)).map((workspace) => workspace.path)).toEqual([
      '/work/b',
    ]);
  });

  it('非 ok 节点没有分片，列表为空', () => {
    const state = reduce(initialClientState, {
      type: 'bootstrap',
      bootstrap: hub(
        [
          snapshot(nodeA, null, 'unreachable', '连接节点失败: 隧道已断开'),
          snapshot(nodeB, null, 'incompatible', 'agent 协议版本 0 与 hub 要求的 1 不一致'),
        ],
        3,
      ),
    });

    expect(state.nodes).toEqual({});
    expect(state.hub.nodes.map((record) => record.status)).toEqual(['unreachable', 'incompatible']);
    expect(state.hub.cursor).toBe(3);
  });
});

describe('侧栏状态', () => {
  it('按 workspace 注册顺序与 session 最近更新时间排序', () => {
    const state = reduce(initialClientState, {
      type: 'bootstrap',
      bootstrap: hub(
        [
          snapshot(
            nodeA,
            bootstrap(
              [workspaceB, workspaceA],
              [
                session('s-old', '/work/a', 100),
                session('s-new', '/work/a', 300),
                session('s-mid', '/work/a', 200),
                session('s-b', '/work/b', 50),
              ],
            ),
          ),
        ],
        7,
      ),
    });

    expect(sidebarWorkspaces(shardOf(state, nodeA))).toEqual([
      {
        id: '/work/a',
        path: '/work/a',
        sessions: [
          {
            id: toSessionId('s-new'),
            title: '标题 s-new',
            state: 'idle',
            queuedCount: 0,
            updatedAt: 300,
            unread: false,
            outcome: null,
          },
          {
            id: toSessionId('s-mid'),
            title: '标题 s-mid',
            state: 'idle',
            queuedCount: 0,
            updatedAt: 200,
            unread: false,
            outcome: null,
          },
          {
            id: toSessionId('s-old'),
            title: '标题 s-old',
            state: 'idle',
            queuedCount: 0,
            updatedAt: 100,
            unread: false,
            outcome: null,
          },
        ],
      },
      {
        id: '/work/b',
        path: '/work/b',
        sessions: [
          {
            id: toSessionId('s-b'),
            title: '标题 s-b',
            state: 'idle',
            queuedCount: 0,
            updatedAt: 50,
            unread: false,
            outcome: null,
          },
        ],
      },
    ]);
  });

  it('由 busy 与队列长度推导忙碌状态', () => {
    const state = reduce(initialClientState, {
      type: 'bootstrap',
      bootstrap: hub(
        [
          snapshot(
            nodeA,
            bootstrap(
              [workspaceA],
              [
                session('s-running', '/work/a', 100, { busy: true }),
                session('s-queued', '/work/a', 200, { queueLength: 2 }),
              ],
            ),
          ),
        ],
        7,
      ),
    });

    expect(sidebarWorkspaces(shardOf(state, nodeA))[0]?.sessions).toEqual([
      {
        id: toSessionId('s-queued'),
        title: '标题 s-queued',
        state: 'queued',
        queuedCount: 2,
        updatedAt: 200,
        unread: false,
        outcome: null,
      },
      {
        id: toSessionId('s-running'),
        title: '标题 s-running',
        state: 'running',
        queuedCount: 0,
        updatedAt: 100,
        unread: false,
        outcome: null,
      },
    ]);
  });

  it('注册与移除 workspace 同步增删其 session', () => {
    const state = loaded();

    const registered = streamed(
      state,
      fact(nodeA, 8, 100, { kind: 'workspace_registered', workspace: workspaceB }),
    );
    expect(sidebarWorkspaces(shardOf(registered, nodeA)).map((workspace) => workspace.path)).toEqual(
      ['/work/a', '/work/b'],
    );

    const withSession = streamed(
      registered,
      fact(nodeA, 9, 101, {
        kind: 'session_created',
        session: session('s-2', '/work/b', 200),
      }),
    );
    const removed = streamed(
      withSession,
      fact(nodeA, 10, 102, { kind: 'workspace_removed', path: '/work/b' }),
    );
    expect(sidebarWorkspaces(shardOf(removed, nodeA)).map((workspace) => workspace.path)).toEqual([
      '/work/a',
    ]);
  });

  it('重命名与删除 session', () => {
    const state = withTree(loaded(), nodeA, tree('s-1', 7, 1, [message(1, null, 'user')]));

    const renamed = streamed(
      state,
      fact(nodeA, 8, 108, {
        kind: 'session_renamed',
        session: session('s-1', '/work/a', 300, { title: '新标题' }),
      }),
    );
    expect(sidebarWorkspaces(shardOf(renamed, nodeA))[0]?.sessions[0]).toEqual({
      id: toSessionId('s-1'),
      title: '新标题',
      state: 'idle',
      queuedCount: 0,
      updatedAt: 300,
      unread: true,
      outcome: null,
    });

    const deleted = streamed(
      renamed,
      fact(nodeA, 9, 109, { kind: 'session_deleted', sessionId: 's-1' }),
    );
    expect(sidebarWorkspaces(shardOf(deleted, nodeA))[0]?.sessions).toEqual([]);
    expect(shardOf(deleted, nodeA).trees['s-1']).toBeUndefined();
    expect(shardOf(deleted, nodeB).trees['s-1']).toBeUndefined();
  });

  it('移除队列项后减少队列长度与队列视图', () => {
    const state = withTree(loaded(), nodeA, {
      ...tree('s-1', 7, null, []),
      session: session('s-1', '/work/a', 100, { queueLength: 1 }),
      queue: [{ id: 5, text: '排队', anchor: null, createdAt: 5, pendingSteer: false }],
    });

    const next = streamed(
      state,
      fact(nodeA, 8, 108, { kind: 'queue_item_removed', sessionId: 's-1', itemId: 5 }),
    );
    expect(shardOf(next, nodeA).sessions[0]?.queueLength).toBe(0);
    expect(sessionQueue(shardOf(next, nodeA), 's-1')).toEqual([]);
  });

  it('插话入队后标记待插入，降级后清除标记', () => {
    const steer = { id: 5, text: '插话', anchor: 3, createdAt: 5, pendingSteer: true };
    const state = withTree(loaded(), nodeA, tree('s-1', 7, null, []));

    const queued = streamed(
      state,
      fact(nodeA, 8, 108, {
        kind: 'message_queued',
        session: session('s-1', '/work/a', 300, { busy: true, queueLength: 1 }),
        item: steer,
      }),
    );
    expect(sessionQueue(shardOf(queued, nodeA), 's-1')).toEqual([steer]);
    expect(shardOf(queued, nodeA).sessions[0]?.queueLength).toBe(1);

    const applied = streamed(
      queued,
      fact(nodeA, 9, 109, {
        kind: 'steer_applied',
        session: session('s-1', '/work/a', 310, { busy: true, queueLength: 0 }),
        runId: 1,
        itemId: 5,
        messageId: 3,
      }),
    );
    expect(shardOf(applied, nodeA).sessions[0]?.queueLength).toBe(0);

    const downgraded = streamed(
      queued,
      fact(nodeA, 9, 109, {
        kind: 'steer_downgraded',
        session: session('s-1', '/work/a', 310, { queueLength: 1 }),
        item: { ...steer, pendingSteer: false },
      }),
    );
    expect(sessionQueue(shardOf(downgraded, nodeA), 's-1')).toEqual([
      { ...steer, pendingSteer: false },
    ]);
  });
});

describe('transcript 推导', () => {
  const nodes = [
    message(1, null, 'user'),
    message(2, 1, 'assistant'),
    message(3, 2, 'user'),
    message(4, 3, 'assistant'),
  ];

  it('取 root 到 active leaf 的路径', () => {
    const state = withTree(loaded(), nodeA, tree('s-1', 7, 4, nodes));

    expect(transcriptNodes(shardOf(state, nodeA), 's-1').map((entry) => entry.id)).toEqual([
      1, 2, 3, 4,
    ]);
    expect(transcriptNodes(shardOf(withTree(loaded(), nodeA, tree('s-1', 7, 2, nodes)), nodeA), 's-1').map((entry) => entry.id)).toEqual([1, 2]);
    expect(transcriptNodes(shardOf(withTree(loaded(), nodeA, tree('s-1', 7, null, nodes)), nodeA), 's-1')).toEqual([]);
  });

  it('切换 active leaf 后 transcript 跟随目标子树', () => {
    const branched = [
      message(1, null, 'user'),
      message(2, 1, 'assistant'),
      message(3, 1, 'user'),
      message(4, 3, 'assistant'),
    ];
    const state = withTree(loaded(), nodeA, tree('s-1', 7, 4, branched));
    expect(transcriptNodes(shardOf(state, nodeA), 's-1').map((entry) => entry.id)).toEqual([
      1, 3, 4,
    ]);

    const switched = streamed(
      state,
      fact(nodeA, 8, 108, {
        kind: 'active_leaf_changed',
        session: session('s-1', '/work/a', 200),
        activeLeaf: 2,
      }),
    );
    expect(transcriptNodes(shardOf(switched, nodeA), 's-1').map((entry) => entry.id)).toEqual([
      1, 2,
    ]);
  });

  it('工具节点在链上但不计入分支兄弟', () => {
    const chained = [
      message(1, null, 'user'),
      message(2, 1, 'assistant', { parts: parts(['tool_call']) }),
      message(3, 2, 'tool', { parts: parts(['tool_result']) }),
      message(4, 2, 'tool', { parts: parts(['tool_result']) }),
      message(5, 3, 'user'),
    ];
    const state = withTree(loaded(), nodeA, tree('s-1', 7, 5, chained));
    const shard = shardOf(state, nodeA);

    expect(transcriptNodes(shard, 's-1').map((entry) => entry.id)).toEqual([1, 2, 3, 5]);
    expect(branchPosition(shard, 's-1', at(chained, 2))).toBeNull();
    expect(branchPosition(shard, 's-1', at(chained, 4))).toBeNull();
  });

  it('分支位置给出兄弟序号与相邻节点', () => {
    const branched = [
      message(1, null, 'user'),
      message(2, 1, 'assistant'),
      message(3, 2, 'user'),
      message(4, 3, 'assistant'),
      message(5, 2, 'user'),
      message(6, 5, 'assistant'),
    ];
    const state = withTree(loaded(), nodeA, tree('s-1', 7, 6, branched));
    const shard = shardOf(state, nodeA);

    expect(transcriptNodes(shard, 's-1').map((entry) => entry.id)).toEqual([1, 2, 5, 6]);
    expect(branchPosition(shard, 's-1', at(branched, 4))).toEqual({
      index: 2,
      total: 2,
      previousId: 3,
      nextId: null,
    });
    expect(branchPosition(shard, 's-1', at(branched, 0))).toBeNull();
  });

  it('压缩消息不计入分支兄弟', () => {
    const compacted = [
      message(1, null, 'user'),
      message(2, 1, 'assistant'),
      message(3, 1, 'compaction'),
    ];
    const state = withTree(loaded(), nodeA, tree('s-1', 7, 2, compacted));
    const shard = shardOf(state, nodeA);

    expect(branchPosition(shard, 's-1', at(compacted, 1))).toBeNull();
    expect(branchPosition(shard, 's-1', at(compacted, 2))).toBeNull();
  });
});

describe('正文缓存', () => {
  it('缺失的正文按 id 批量请求，同一 id 不重复请求', () => {
    const state = withTree(loaded(), nodeA, tree('s-1', 7, 2, [
      message(1, null, 'user'),
      message(2, 1, 'assistant'),
    ]));
    const shard = shardOf(state, nodeA);

    expect(missingBodyIds(shard, 's-1')).toEqual([1, 2]);

    const requested = reduce(state, { type: 'bodies-request', node: nodeA, ids: [1, 2] });
    expect(missingBodyIds(shardOf(requested, nodeA), 's-1')).toEqual([]);
  });

  it('拉到的正文按消息 id 缓存，未返回的 id 记为空正文', () => {
    const state = reduce(
      withTree(loaded(), nodeA, tree('s-1', 7, 1, [message(1, null, 'user')])),
      { type: 'bodies-request', node: nodeA, ids: [1, 2] },
    );
    const loadedBodies = reduce(state, {
      type: 'bodies',
      node: nodeA,
      ids: [1, 2],
      messages: [{ id: 1, parts: text('问') }],
    });
    const shard = shardOf(loadedBodies, nodeA);

    expect(bodyOf(shard, 1)).toEqual(text('问'));
    expect(bodyOf(shard, 2)).toEqual([]);
    expect(bodyOf(shard, 3)).toBeNull();
    expect(shard.bodyRequests).toEqual([]);
  });

  it('message_updated 使该消息的正文失效，其它消息不变', () => {
    const state = reduce(withTree(loaded(), nodeA, tree('s-1', 7, 3, [
      message(1, null, 'user'),
      message(2, 1, 'assistant', { parts: parts(['tool_call']) }),
      message(3, 2, 'tool', { parts: parts(['tool_result']) }),
    ])), { type: 'bodies-request', node: nodeA, ids: [2, 3] });
    const withBodies = reduce(state, {
      type: 'bodies',
      node: nodeA,
      ids: [2, 3],
      messages: [
        { id: 2, parts: [{ kind: 'tool_call', callId: 'call_1', name: 'read_file', arguments: '{}', providerMeta: null }] },
        { id: 3, parts: [{ kind: 'tool_result', callId: 'call_1', state: { status: 'running' }, providerMeta: null }] },
      ],
    });

    const updated = streamed(
      withBodies,
      fact(nodeA, 8, 108, {
        kind: 'message_updated',
        session: session('s-1', '/work/a', 200),
        message: message(3, 2, 'tool', { parts: parts(['tool_result']) }),
      }),
    );
    const shard = shardOf(updated, nodeA);

    expect(bodyOf(shard, 2)).not.toBeNull();
    expect(bodyOf(shard, 3)).toBeNull();
    expect(missingBodyIds(shard, 's-1')).toEqual([1, 3]);
  });

  it('拉取失败的消息不再自动重试，快照重载后恢复', () => {
    const state = withTree(loaded(), nodeA, tree('s-1', 7, 1, [message(1, null, 'user')]));
    const requested = reduce(state, { type: 'bodies-request', node: nodeA, ids: [1] });
    const failed = reduce(requested, { type: 'bodies-failed', node: nodeA, ids: [1] });

    expect(missingBodyIds(shardOf(failed, nodeA), 's-1')).toEqual([]);

    const reloaded = withTree(failed, nodeA, tree('s-1', 7, 1, [message(1, null, 'user')]));
    expect(missingBodyIds(shardOf(reloaded, nodeA), 's-1')).toEqual([1]);
  });

  it('快照重载清空在途请求，被中断的正文可以重新拉取', () => {
    const state = withTree(loaded(), nodeA, tree('s-1', 7, 1, [message(1, null, 'user')]));
    const requested = reduce(state, { type: 'bodies-request', node: nodeA, ids: [1] });
    expect(missingBodyIds(shardOf(requested, nodeA), 's-1')).toEqual([]);

    const reloaded = withTree(requested, nodeA, tree('s-1', 7, 1, [message(1, null, 'user')]));
    expect(missingBodyIds(shardOf(reloaded, nodeA), 's-1')).toEqual([1]);
  });

  it('失效后迟到的正文响应被丢弃，并重新拉取', () => {
    const state = withTree(loaded(), nodeA, tree('s-1', 7, 2, [
      message(1, null, 'user'),
      message(2, 1, 'tool', { parts: parts(['tool_result']) }),
    ]));
    const requested = reduce(state, { type: 'bodies-request', node: nodeA, ids: [2] });
    const invalidated = streamed(
      requested,
      fact(nodeA, 8, 108, {
        kind: 'message_updated',
        session: session('s-1', '/work/a', 200),
        message: message(2, 1, 'tool', { parts: parts(['tool_result']) }),
      }),
    );
    const late = reduce(invalidated, {
      type: 'bodies',
      node: nodeA,
      ids: [2],
      messages: [
        { id: 2, parts: [{ kind: 'tool_result', callId: 'call_1', state: { status: 'running' }, providerMeta: null }] },
      ],
    });
    const shard = shardOf(late, nodeA);

    expect(bodyOf(shard, 2)).toBeNull();
    expect(missingBodyIds(shard, 's-1')).toEqual([1, 2]);
  });

  it('拉取失败的消息可以显式重试', () => {
    const state = withTree(loaded(), nodeA, tree('s-1', 7, 1, [message(1, null, 'user')]));
    const failed = reduce(
      reduce(state, { type: 'bodies-request', node: nodeA, ids: [1] }),
      { type: 'bodies-failed', node: nodeA, ids: [1] },
    );
    expect(missingBodyIds(shardOf(failed, nodeA), 's-1')).toEqual([]);

    const retried = reduce(failed, { type: 'bodies-retry', node: nodeA, ids: [1] });
    expect(missingBodyIds(shardOf(retried, nodeA), 's-1')).toEqual([1]);
  });

  it('删除 session 时丢弃它的正文缓存', () => {
    const state = reduce(
      reduce(withTree(loaded(), nodeA, tree('s-1', 7, 1, [message(1, null, 'user')])), {
        type: 'bodies-request',
        node: nodeA,
        ids: [1],
      }),
      {
        type: 'bodies',
        node: nodeA,
        ids: [1],
        messages: [{ id: 1, parts: text('问') }],
      },
    );
    const deleted = streamed(
      state,
      fact(nodeA, 8, 108, { kind: 'session_deleted', sessionId: 's-1' }),
    );
    const shard = shardOf(deleted, nodeA);

    expect(bodyOf(shard, 1)).toBeNull();
    expect(shard.bodyRequests).toEqual([]);
    expect(shard.bodyFailures).toEqual([]);
  });

  it('工具结局按 call id 汇总，正文未到时没有条目', () => {
    const state = withTree(loaded(), nodeA, tree('s-1', 7, 3, [
      message(1, null, 'user'),
      message(2, 1, 'assistant', { parts: parts(['tool_call']) }),
      message(3, 2, 'tool', { parts: parts(['tool_result']) }),
    ]));
    expect(toolResults(shardOf(state, nodeA), 's-1')).toEqual({});

    const withBody = reduce(
      reduce(state, { type: 'bodies-request', node: nodeA, ids: [3] }),
      {
      type: 'bodies',
      node: nodeA,
      ids: [3],
      messages: [
        { id: 3, parts: [{ kind: 'tool_result', callId: 'call_1', state: { status: 'unknown' }, providerMeta: null }] },
      ],
      },
    );
    expect(toolResults(shardOf(withBody, nodeA), 's-1')).toEqual({
      call_1: { status: 'unknown' },
    });
  });
});

describe('流式增量与占位节点', () => {
  const asked = (): ClientState =>
    withTree(loaded(), nodeA, tree('s-1', 7, 1, [message(1, null, 'user')]));

  it('未定型的增量建本地占位，并合并同类块', () => {
    let state = streamed(asked(), delta(nodeA, 's-1', 9, '你'));
    state = streamed(state, delta(nodeA, 's-1', 9, '好'));
    state = streamed(state, delta(nodeA, 's-1', 9, '想', 'reasoning'));

    expect(placeholdersOf(shardOf(state, nodeA), 's-1')).toEqual([
      {
        id: 9,
        parts: [
          { kind: 'text', text: '你好', providerMeta: null },
          { kind: 'reasoning', text: '想', providerMeta: null },
        ],
      },
    ]);
    expect(transcriptNodes(shardOf(state, nodeA), 's-1').map((entry) => entry.id)).toEqual([1]);
  });

  it('message_appended 用同一 id 的骨架接管占位', () => {
    const streaming = streamed(asked(), delta(nodeA, 's-1', 9, '部分'));
    const appended = streamed(
      streamed(
        streaming,
        fact(nodeA, 9, 109, {
          kind: 'message_appended',
          session: session('s-1', '/work/a', 200, { busy: true }),
          message: message(9, 1, 'assistant', { runId: 1, parts: parts(['text']) }),
        }),
      ),
      fact(nodeA, 10, 110, {
        kind: 'active_leaf_changed',
        session: session('s-1', '/work/a', 210, { busy: true }),
        activeLeaf: 9,
      }),
    );

    expect(placeholdersOf(shardOf(appended, nodeA), 's-1')).toEqual([]);
    expect(missingBodyIds(shardOf(appended, nodeA), 's-1')).toEqual([1, 9]);
  });

  it('run 结束时丢弃未定型的占位', () => {
    const running: RunRecord = {
      id: 1,
      sessionId: 's-1',
      status: 'running',
      failure: null,
      createdAt: 100,
      updatedAt: 100,
    };
    let state = streamed(
      asked(),
      fact(nodeA, 8, 108, {
        kind: 'run_started',
        session: session('s-1', '/work/a', 200, { busy: true }),
        run: running,
      }),
    );
    state = streamed(state, delta(nodeA, 's-1', 9, '被打断的回答。'));
    expect(placeholdersOf(shardOf(state, nodeA), 's-1')).toHaveLength(1);

    state = streamed(
      state,
      fact(nodeA, 9, 109, {
        kind: 'run_finished',
        session: session('s-1', '/work/a', 300),
        run: { ...running, status: 'cancelled', updatedAt: 300 },
      }),
    );
    expect(placeholdersOf(shardOf(state, nodeA), 's-1')).toEqual([]);
  });

  it('快照重载丢弃占位', () => {
    const streaming = streamed(asked(), delta(nodeA, 's-1', 9, '部分'));
    const reloaded = withTree(streaming, nodeA, tree('s-1', 7, 1, [message(1, null, 'user')]));

    expect(placeholdersOf(shardOf(reloaded, nodeA), 's-1')).toEqual([]);
  });

  it('已入树的同一 id 不再累积增量', () => {
    const state = streamed(
      withTree(loaded(), nodeA, tree('s-1', 7, 2, [
        message(1, null, 'user'),
        message(2, 1, 'assistant', { runId: 1 }),
      ])),
      delta(nodeA, 's-1', 2, '迟到增量'),
    );
    expect(placeholdersOf(shardOf(state, nodeA), 's-1')).toEqual([]);
  });

  it('delta 不推进广播游标', () => {
    const state = streamed(asked(), delta(nodeA, 's-1', 9, '你'));
    expect(state.hub.cursor).toBe(7);
  });

  it('由 run 状态推导 assistant 的展示状态与失败说明', () => {
    const run: RunRecord = {
      id: 1,
      sessionId: 's-1',
      status: 'running',
      failure: null,
      createdAt: 100,
      updatedAt: 100,
    };
    let state = streamed(
      withTree(loaded(), nodeA, tree('s-1', 7, 2, [
        message(1, null, 'user'),
        message(2, 1, 'assistant', { runId: 1 }),
      ])),
      fact(nodeA, 8, 108, {
        kind: 'run_started',
        session: session('s-1', '/work/a', 200, { busy: true }),
        run,
      }),
    );
    const assistant = at(shardOf(state, nodeA).trees['s-1']?.nodes ?? [], 1);
    expect(messageView(shardOf(state, nodeA), 's-1', assistant)).toEqual({
      state: 'streaming',
      note: null,
    });

    state = streamed(
      state,
      fact(nodeA, 9, 109, {
        kind: 'run_finished',
        session: session('s-1', '/work/a', 300),
        run: {
          ...run,
          status: 'failed',
          failure: { code: 'model_upstream', message: '上游 502' },
          updatedAt: 300,
        },
      }),
    );
    expect(messageView(shardOf(state, nodeA), 's-1', assistant)).toEqual({
      state: 'failed',
      note: '模型上游错误：上游 502',
    });
  });
});

describe('分片隔离', () => {
  it('快照游标取较老水位，无基线时取快照水位', () => {
    expect(snapshotCursor(null, 7)).toBe(7);
    expect(snapshotCursor(9, 7)).toBe(7);
    expect(snapshotCursor(5, 9)).toBe(5);
  });

  it('重复到达的事实只应用一次', () => {
    const state = withTree(loaded(), nodeA, tree('s-1', 7, 1, [message(1, null, 'user')]));
    const event = fact(nodeA, 8, 108, {
      kind: 'session_renamed',
      session: session('s-1', '/work/a', 200, { title: '改名' }),
    });

    const once = streamed(state, event);
    const twice = streamed(once, event);
    expect(twice).toBe(once);
    expect(shardOf(twice, nodeA).sessions[0]?.title).toBe('改名');
  });

  it('乱序到达的旧事实不改变已有状态', () => {
    const newer = streamed(
      loaded(),
      fact(nodeA, 8, 110, {
        kind: 'session_renamed',
        session: session('s-1', '/work/a', 300, { title: '新' }),
      }),
    );
    const older = streamed(
      newer,
      fact(nodeA, 9, 105, {
        kind: 'session_renamed',
        session: session('s-1', '/work/a', 200, { title: '旧' }),
      }),
    );

    expect(shardOf(older, nodeA).sessions[0]?.title).toBe('新');
  });

  it('两节点同名 session 的占位互不串写', () => {
    let state = withTree(
      withTree(loaded(), nodeA, tree('s-1', 7, 2, [
        message(1, null, 'user'),
        message(2, 1, 'assistant', { runId: 1 }),
      ])),
      nodeB,
      tree('s-1', 7, 5, [message(5, null, 'assistant', { runId: 1 })]),
    );

    state = streamed(state, delta(nodeB, 's-1', 6, 'B 输出'));
    state = streamed(state, delta(nodeA, 's-1', 2, 'A 输出'));

    expect(placeholdersOf(shardOf(state, nodeB), 's-1')).toEqual([
      { id: 6, parts: [{ kind: 'text', text: 'B 输出', providerMeta: null }] },
    ]);
    expect(placeholdersOf(shardOf(state, nodeA), 's-1')).toEqual([]);
  });

  it('两节点事件交错到达时各自分片互不影响', () => {
    const before = loaded();
    const state = streamed(
      streamed(
        before,
        fact(nodeA, 8, 108, {
          kind: 'session_created',
          session: session('s-2', '/work/a', 200),
        }),
      ),
      fact(nodeB, 9, 208, {
        kind: 'session_created',
        session: session('s-3', '/work/b', 400),
      }),
    );

    expect(shardOf(state, nodeA).sessions.map((entry) => entry.id)).toEqual(['s-1', 's-2']);
    expect(shardOf(state, nodeB).sessions.map((entry) => entry.id)).toEqual(['s-1', 's-3']);
  });

  it('分片建立前到达的事实只推进广播游标', () => {
    const pending = reduce(initialClientState, {
      type: 'bootstrap',
      bootstrap: hub([snapshot(nodeA, null, 'unreachable', '连接节点失败')], 4),
    });
    const ok = streamed(pending, {
      type: 'node_status_changed',
      hubSeq: 5,
      name: nodeA,
      status: 'ok',
      detail: null,
    });
    const advanced = streamed(
      ok,
      fact(nodeA, 6, 108, {
        kind: 'session_created',
        session: session('s-1', '/work/a', 108),
      }),
    );

    expect(advanced.nodes[nodeA]).toBeUndefined();
    expect(advanced.hub.cursor).toBe(6);

    const bootstrapped = reduce(advanced, {
      type: 'node-bootstrap',
      node: nodeA,
      bootstrap: bootstrap([workspaceA], [session('s-1', '/work/a', 108)], 108),
    });
    expect(sidebarWorkspaces(shardOf(bootstrapped, nodeA))).toHaveLength(1);
    expect(shardOf(bootstrapped, nodeA).lastEventId).toBe(108);
  });

  it('切换选中节点不改变任何分片', () => {
    const state = withTree(loaded(), nodeA, tree('s-1', 7, 1, [message(1, null, 'user')]));
    const switched = reduce(state, { type: 'select-node', node: nodeB });

    expect(switched.hub.selected).toBe(nodeB);
    expect(switched.nodes).toBe(state.nodes);
    expect(switched.hub.cursor).toBe(state.hub.cursor);
  });

  it('广播序号重复的事件被忽略', () => {
    const state = loaded();
    const once = streamed(
      state,
      fact(nodeA, 8, 108, {
        kind: 'session_renamed',
        session: session('s-1', '/work/a', 200, { title: '新' }),
      }),
    );
    const replayed = streamed(
      once,
      fact(nodeB, 8, 208, {
        kind: 'session_renamed',
        session: session('s-1', '/work/b', 400, { title: '串写' }),
      }),
    );

    expect(replayed).toBe(once);
    expect(shardOf(replayed, nodeB).sessions[0]?.title).toBe('标题 s-1');
  });
});

describe('节点表变更', () => {
  it('node_registered 增加不可达记录，node_status_changed 标记可用', () => {
    const registered = streamed(loaded(), {
      type: 'node_registered',
      hubSeq: 8,
      node: { name: 'legacy-lab', url: 'http://127.0.0.1:7081', createdAt: 40 },
    });
    expect(registered.hub.nodes.at(-1)).toEqual({
      name: 'legacy-lab',
      url: 'http://127.0.0.1:7081',
      status: 'unreachable',
      detail: null,
    });

    const ok = streamed(registered, {
      type: 'node_status_changed',
      hubSeq: 9,
      name: 'legacy-lab',
      status: 'ok',
      detail: null,
    });
    expect(ok.hub.nodes.at(-1)?.status).toBe('ok');
    expect(ok.nodes['legacy-lab']).toBeUndefined();

    const bootstrapped = reduce(ok, {
      type: 'node-bootstrap',
      node: 'legacy-lab',
      bootstrap: bootstrap([workspaceA], [session('s-9', '/work/a', 50)], 3),
    });
    expect(sidebarWorkspaces(shardOf(bootstrapped, 'legacy-lab'))).toHaveLength(1);
  });

  it('节点转为非 ok 时清空其分片，其它分片不受影响', () => {
    const state = streamed(loaded(), {
      type: 'node_status_changed',
      hubSeq: 8,
      name: nodeB,
      status: 'unreachable',
      detail: '连接节点失败: 隧道已断开',
    });

    expect(state.nodes[nodeB]).toBeUndefined();
    expect(state.hub.nodes.find((record) => record.name === nodeB)).toEqual({
      name: nodeB,
      url: `http://127.0.0.1:${nodeB}`,
      status: 'unreachable',
      detail: '连接节点失败: 隧道已断开',
    });
    expect(sidebarWorkspaces(shardOf(state, nodeA))).not.toEqual([]);
  });

  it('改名按删除加登记广播，旧名与旧分片一起消失', () => {
    const removed = streamed(loaded(), { type: 'node_removed', hubSeq: 8, name: nodeB });
    const registered = streamed(removed, {
      type: 'node_registered',
      hubSeq: 9,
      node: { name: 'studio-2', url: `http://127.0.0.1:${nodeB}`, createdAt: 20 },
    });

    expect(registered.hub.nodes.map((record) => record.name)).toEqual([nodeA, 'studio-2']);
    expect(registered.hub.nodes.at(-1)?.status).toBe('unreachable');
    expect(registered.nodes[nodeB]).toBeUndefined();
    expect(registered.hub.cursor).toBe(9);
  });

  it('node_removed 删除记录与分片', () => {
    const state = reduce(loaded(), { type: 'select-node', node: nodeB });
    const removed = streamed(state, { type: 'node_removed', hubSeq: 8, name: nodeB });

    expect(removed.hub.nodes.map((record) => record.name)).toEqual([nodeA]);
    expect(removed.nodes[nodeB]).toBeUndefined();
    expect(removed.hub.selected).toBe(nodeB);
  });

  it('resync 事件只提示重连，不改变视图状态', () => {
    const state = loaded();
    const after = streamed(state, { type: 'resync', reason: { kind: 'lagged', skipped: 3 } });
    expect(after).toBe(state);
  });
});

describe('快照水位与回放恢复', () => {
  const question = message(1, null, 'user');
  const answer = message(2, 1, 'assistant', { runId: 1 });

  it('tree 快照落后于已应用的事实时补齐尾部', () => {
    const appended = streamed(
      streamed(
        loaded(),
        fact(nodeA, 8, 108, {
          kind: 'message_appended',
          session: session('s-1', '/work/a', 108),
          message: question,
        }),
      ),
      fact(nodeA, 9, 109, {
        kind: 'message_appended',
        session: session('s-1', '/work/a', 109),
        message: answer,
      }),
    );
    expect(shardOf(appended, nodeA).lastEventId).toBe(109);

    const landed = withTree(appended, nodeA, tree('s-1', 108, null, [question]));
    const shard = shardOf(landed, nodeA);

    expect(shard.lastEventId).toBe(109);
    expect(shard.trees['s-1']?.nodes.map((entry) => entry.id)).toEqual([1, 2]);
    expect(shard.trees['s-1']?.activeLeaf).toBeNull();
    expect(landed.hub.cursor).toBe(9);
  });

  it('tree 快照水位高于分片水位时不推进水位', () => {
    const state = streamed(
      loaded(),
      fact(nodeA, 8, 108, {
        kind: 'session_renamed',
        session: session('s-1', '/work/a', 108, { title: '新' }),
      }),
    );
    const landed = withTree(state, nodeA, tree('s-1', 400, null, []));

    expect(shardOf(landed, nodeA).lastEventId).toBe(108);
    expect(shardOf(landed, nodeA).sessions.map((entry) => entry.id)).toEqual(['s-1']);
  });

  it('一个节点的快照不改变另一节点的分片', () => {
    const state = streamed(
      loaded(),
      fact(nodeB, 8, 308, {
        kind: 'session_renamed',
        session: session('s-1', '/work/b', 308, { title: 'B 改名' }),
      }),
    );
    const landed = withTree(state, nodeA, tree('s-1', 7, null, []));

    expect(shardOf(landed, nodeB).sessions[0]?.title).toBe('B 改名');
    expect(shardOf(landed, nodeB).lastEventId).toBe(308);
  });

  it('回放事件构建的 transcript 等于 tree 快照', () => {
    const base = loaded();
    let replayed = streamed(
      base,
      fact(nodeA, 8, 108, {
        kind: 'message_appended',
        session: session('s-1', '/work/a', 108),
        message: question,
      }),
    );
    replayed = streamed(
      replayed,
      fact(nodeA, 9, 109, {
        kind: 'message_appended',
        session: session('s-1', '/work/a', 109),
        message: answer,
      }),
    );
    replayed = streamed(
      replayed,
      fact(nodeA, 10, 110, {
        kind: 'active_leaf_changed',
        session: session('s-1', '/work/a', 110),
        activeLeaf: 2,
      }),
    );

    const snapshot = withTree(base, nodeA, tree('s-1', 110, 2, [question, answer]));

    expect(transcriptNodes(shardOf(replayed, nodeA), 's-1')).toEqual(
      transcriptNodes(shardOf(snapshot, nodeA), 's-1'),
    );
  });

  it('加载快照后重放新事实等于从零构建', () => {
    const base = loaded();
    const append = fact(nodeA, 8, 108, {
      kind: 'message_appended',
      session: session('s-1', '/work/a', 108),
      message: question,
    });
    const leaf = fact(nodeA, 9, 109, {
      kind: 'active_leaf_changed',
      session: session('s-1', '/work/a', 109),
      activeLeaf: 1,
    });

    const live = streamed(streamed(base, append), leaf);
    const landed = withTree(live, nodeA, tree('s-1', 107, null, []));
    const fromScratch = streamed(
      streamed(withTree(base, nodeA, tree('s-1', 107, null, [])), append),
      leaf,
    );

    expect(shardOf(landed, nodeA).trees).toEqual(shardOf(fromScratch, nodeA).trees);
  });
});

describe('连接状态与错误', () => {
  it('记录连接状态与错误横幅', () => {
    const connected = reduce(initialClientState, { type: 'connection', connection: 'connected' });
    expect(connected.connection).toBe('connected');

    const failed = reduce(connected, {
      type: 'error',
      error: { code: 'node_unavailable', message: '节点不可用' },
    });
    expect(failed.error).toEqual({ code: 'node_unavailable', message: '节点不可用' });

    expect(reduce(failed, { type: 'dismiss-error' }).error).toBeNull();
  });
});

describe('分片辅助', () => {
  it('loadSessionTree 直接作用于分片', () => {
    const shard = loadSessionTree(shardOf(loaded(), nodeA), tree('s-2', 9, null, []));
    expect(shard.trees['s-2']?.activeLeaf).toBeNull();
    expect(shard.lastEventId).toBe(7);
  });

  it('正文请求只作用于已有分片', () => {
    const state = loaded();
    const untouched = reduce(state, { type: 'bodies-request', node: 'ghost', ids: [1] });
    expect(untouched).toBe(state);
  });
});

describe('供应商限流重试', () => {
  const runningRun: RunRecord = {
    id: 1,
    sessionId: 's-1',
    status: 'running',
    failure: null,
    createdAt: 100,
    updatedAt: 100,
  };

  function retryEvent(
    node: string,
    sessionId: string,
    attempt = 1,
    maxAttempts = 3,
    delayMs = 2000,
  ): HubStreamEvent {
    return {
      type: 'retry',
      node,
      sessionId: toSessionId(sessionId),
      attempt,
      maxAttempts,
      delayMs,
    };
  }

  function withRunning(): ClientState {
    const state = withTree(loaded(), nodeA, tree('s-1', 7, 2, [
      message(1, null, 'user'),
      message(2, 1, 'assistant', { runId: 1 }),
    ]));
    return streamed(
      state,
      fact(nodeA, 8, 108, {
        kind: 'run_started',
        session: session('s-1', '/work/a', 200, { busy: true }),
        run: runningRun,
      }),
    );
  }

  function assistantOf(state: ClientState): MessageSkeleton {
    return at(shardOf(state, nodeA).trees['s-1']?.nodes ?? [], 1);
  }

  it('按节点与 session 记录重试等待，不推进广播游标', () => {
    const state = withRunning();
    const cursor = state.hub.cursor;

    const retrying = streamed(state, retryEvent(nodeA, 's-1'));

    expect(shardOf(retrying, nodeA).retries['s-1']).toEqual({
      attempt: 1,
      maxAttempts: 3,
      delayMs: 2000,
    });
    expect(shardOf(retrying, nodeB).retries).toEqual({});
    expect(retrying.hub.cursor).toBe(cursor);
    expect(streamed(retrying, retryEvent('ghost', 's-1'))).toBe(retrying);
  });

  it('重试期间 assistant 显示等待说明，文本重新流动后消失', () => {
    const retrying = streamed(withRunning(), retryEvent(nodeA, 's-1'));
    expect(messageView(shardOf(retrying, nodeA), 's-1', assistantOf(retrying))).toEqual({
      state: 'streaming',
      note: '供应商限流，第 1/3 次重试，等待 2 秒',
    });

    const streaming = streamed(retrying, delta(nodeA, 's-1', 2, '恢复'));
    expect(messageView(shardOf(streaming, nodeA), 's-1', assistantOf(streaming))).toEqual({
      state: 'streaming',
      note: null,
    });
  });

  it('等待时长小于一秒时按毫秒显示', () => {
    const retrying = streamed(withRunning(), retryEvent(nodeA, 's-1', 2, 3, 250));
    expect(messageView(shardOf(retrying, nodeA), 's-1', assistantOf(retrying))).toEqual({
      state: 'streaming',
      note: '供应商限流，第 2/3 次重试，等待 250 毫秒',
    });
  });

  it('run 结束与 session 树重载都会清除重试等待', () => {
    const retrying = streamed(withRunning(), retryEvent(nodeA, 's-1'));
    const finished = streamed(
      retrying,
      fact(nodeA, 9, 109, {
        kind: 'run_finished',
        session: session('s-1', '/work/a', 300),
        run: {
          ...runningRun,
          status: 'failed',
          failure: { code: 'model_rate_limited', message: '模型返回非成功状态 429: 限流' },
          updatedAt: 300,
        },
      }),
    );

    expect(shardOf(finished, nodeA).retries['s-1']).toBeUndefined();
    expect(messageView(shardOf(finished, nodeA), 's-1', assistantOf(finished))).toEqual({
      state: 'failed',
      note: '供应商限流：模型返回非成功状态 429: 限流',
    });

    const reloaded = withTree(
      retrying,
      nodeA,
      tree('s-1', 9, 2, [
        message(1, null, 'user'),
        message(2, 1, 'assistant', { runId: 1 }),
      ]),
    );
    expect(shardOf(reloaded, nodeA).retries['s-1']).toBeUndefined();
  });
});
