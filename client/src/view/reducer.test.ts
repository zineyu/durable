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
import type { SessionTreeSnapshot } from '../api/generated/SessionTreeSnapshot';
import type { WorkspaceMetadata } from '../api/generated/WorkspaceMetadata';
import { toSessionId } from './ids';
import { missingBodyIds, sessionView } from './derived';
import { loadSessionTree, snapshotCursor } from './node-state';
import type { NodeState } from './node-state';
import type { MessagePlaceholder } from './types';
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
    interrupted: false,
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
  return {
    name,
    url: `http://127.0.0.1:${name}`,
    enabled: true,
    status,
    detail,
    lastEventAt: null,
    latencyMs: null,
    bootstrap: node,
  };
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

function fact(
  node: string,
  hubSeq: number,
  id: number,
  event: FactEvent,
  session: SessionMetadata | null = null,
): HubStreamEvent {
  if (session !== null && 'sessionId' in event && event.sessionId !== session.id) {
    throw new Error('夹具的 sessionId 必须与信封的 session 一致');
  }
  return {
    type: 'fact',
    hubSeq,
    node,
    id,
    sessionId: session === null ? null : session.id,
    session,
    timestamp: id,
    event,
  };
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

/// 正文缓存是分片字段，归约用例直接读分片；呈现侧读它的是 `derived.ts`。
function bodyOf(shard: NodeState, messageId: number): ContentPart[] | null {
  return shard.bodies[messageId] ?? null;
}

function placeholdersOf(shard: NodeState, sessionId: string): MessagePlaceholder[] {
  return shard.placeholders[sessionId] ?? [];
}

/// 派生视图的 transcript 顺序，用于断言两条事件路径得到同一结果。
function transcriptIds(state: ClientState, node: string): number[] {
  const view = sessionView(shardOf(state, node), 's-1');
  return view === null ? [] : view.transcript.map((entry) => entry.message.id);
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
): SessionTreeSnapshot {
  return {
    maxEventId,
    session: session(id, '/work/a', maxEventId),
    activeLeaf,
    nodes,
    queue: [],
    runs: [],
  };
}

function withTree(state: ClientState, node: string, value: SessionTreeSnapshot): ClientState {
  return reduce(state, { type: 'tree', node, tree: value });
}

describe('合并 bootstrap', () => {
  it('按节点建立分片并记录广播游标', () => {
    const state = loaded();

    expect(state.hub.nodes.map((record) => record.name)).toEqual([nodeA, nodeB]);
    expect(state.hub.cursor).toBe(7);
    expect(shardOf(state, nodeA).modelName).toBe('gpt-5-codex');
    expect(shardOf(state, nodeA).lastEventId).toBe(7);
    expect(shardOf(state, nodeB).workspaces.map((workspace) => workspace.path)).toEqual([
      '/work/b',
    ]);
  });

  it('hub 层只有节点表与广播游标，选中位置不进入 reducer', () => {
    expect(Object.keys(loaded().hub).sort()).toEqual(['cursor', 'nodes']);
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
  it('注册与移除 workspace 同步增删其 session', () => {
    const state = loaded();

    const registered = streamed(
      state,
      fact(nodeA, 8, 100, { kind: 'workspace_registered', workspace: workspaceB }),
    );
    expect(shardOf(registered, nodeA).workspaces.map((workspace) => workspace.path)).toEqual([
      '/work/a',
      '/work/b',
    ]);

    const withSession = streamed(
      registered,
      fact(nodeA, 9, 101, { sessionId: 's-2', kind: 'session_created' }, session('s-2', '/work/b', 200)),
    );
    const removed = streamed(
      withSession,
      fact(nodeA, 10, 102, { kind: 'workspace_removed', path: '/work/b' }),
    );
    expect(shardOf(removed, nodeA).workspaces.map((workspace) => workspace.path)).toEqual([
      '/work/a',
    ]);
  });

  it('重命名与删除 session', () => {
    const state = withTree(loaded(), nodeA, tree('s-1', 7, 1, [message(1, null, 'user')]));

    const renamed = streamed(
      state,
      fact(nodeA, 8, 108, { sessionId: 's-1', kind: 'session_renamed' }, session('s-1', '/work/a', 300, { title: '新标题' })),
    );
    expect(shardOf(renamed, nodeA).sessions[0]).toEqual(
      session('s-1', '/work/a', 300, { title: '新标题' }),
    );

    const deleted = streamed(
      renamed,
      fact(nodeA, 9, 109, { kind: 'session_deleted', sessionId: 's-1' }),
    );
    expect(shardOf(deleted, nodeA).sessions).toEqual([]);
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
      fact(
        nodeA,
        8,
        108,
        { kind: 'queue_item_removed', sessionId: 's-1', itemId: 5 },
        session('s-1', '/work/a', 108, { queueLength: 0 }),
      ),
    );
    expect(shardOf(next, nodeA).sessions[0]?.queueLength).toBe(0);
    expect(shardOf(next, nodeA).trees['s-1']?.queue).toEqual([]);
  });

  it('迁移前的队列项移除没有 session 视图时只删队列项', () => {
    const state = withTree(loaded(), nodeA, {
      ...tree('s-1', 7, null, []),
      session: session('s-1', '/work/a', 100, { queueLength: 1 }),
      queue: [{ id: 5, text: '排队', anchor: null, createdAt: 5, pendingSteer: false }],
    });

    const next = streamed(
      state,
      fact(nodeA, 8, 108, { kind: 'queue_item_removed', sessionId: 's-1', itemId: 5 }),
    );
    expect(shardOf(next, nodeA).trees['s-1']?.queue).toEqual([]);
    expect(shardOf(next, nodeA).sessions[0]?.queueLength).toBe(1);
  });

  it('插话入队后标记待插入，降级后清除标记', () => {
    const steer = { id: 5, text: '插话', anchor: 3, createdAt: 5, pendingSteer: true };
    const state = withTree(loaded(), nodeA, tree('s-1', 7, null, []));

    const queued = streamed(
      state,
      fact(nodeA, 8, 108, {
        sessionId: 's-1',
        kind: 'message_queued',
        item: steer,
      }, session('s-1', '/work/a', 300, { busy: true, queueLength: 1 })),
    );
    expect(shardOf(queued, nodeA).trees['s-1']?.queue).toEqual([steer]);
    expect(shardOf(queued, nodeA).sessions[0]?.queueLength).toBe(1);

    const applied = streamed(
      queued,
      fact(nodeA, 9, 109, {
        sessionId: 's-1',
        kind: 'steer_applied',
        runId: 1,
        itemId: 5,
        messageId: 3,
      }, session('s-1', '/work/a', 310, { busy: true, queueLength: 0 })),
    );
    expect(shardOf(applied, nodeA).sessions[0]?.queueLength).toBe(0);

    const downgraded = streamed(
      queued,
      fact(nodeA, 9, 109, {
        sessionId: 's-1',
        kind: 'steer_downgraded',
        item: { ...steer, pendingSteer: false },
      }, session('s-1', '/work/a', 310, { queueLength: 1 })),
    );
    expect(shardOf(downgraded, nodeA).trees['s-1']?.queue).toEqual([
      { ...steer, pendingSteer: false },
    ]);
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
        sessionId: 's-1',
        kind: 'message_updated',
        message: message(3, 2, 'tool', { parts: parts(['tool_result']) }),
      }, session('s-1', '/work/a', 200)),
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
        sessionId: 's-1',
        kind: 'message_updated',
        message: message(2, 1, 'tool', { parts: parts(['tool_result']) }),
      }, session('s-1', '/work/a', 200)),
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
    expect(shardOf(state, nodeA).trees['s-1']?.nodes.map((node) => node.id)).toEqual([1]);
  });

  it('message_appended 用同一 id 的骨架接管占位', () => {
    const streaming = streamed(asked(), delta(nodeA, 's-1', 9, '部分'));
    const appended = streamed(
      streamed(
        streaming,
        fact(nodeA, 9, 109, {
          sessionId: 's-1',
          kind: 'message_appended',
          message: message(9, 1, 'assistant', { runId: 1, parts: parts(['text']) }),
        }, session('s-1', '/work/a', 200, { busy: true })),
      ),
      fact(nodeA, 10, 110, {
        sessionId: 's-1',
        kind: 'active_leaf_changed',
        activeLeaf: 9,
      }, session('s-1', '/work/a', 210, { busy: true })),
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
      progress: null,
      createdAt: 100,
      updatedAt: 100,
    };
    let state = streamed(
      asked(),
      fact(nodeA, 8, 108, {
        sessionId: 's-1',
        kind: 'run_started',
        run: running,
      }, session('s-1', '/work/a', 200, { busy: true })),
    );
    state = streamed(state, delta(nodeA, 's-1', 9, '被打断的回答。'));
    expect(placeholdersOf(shardOf(state, nodeA), 's-1')).toHaveLength(1);

    state = streamed(
      state,
      fact(nodeA, 9, 109, {
        sessionId: 's-1',
        kind: 'run_finished',
        run: { ...running, status: 'cancelled', updatedAt: 300 },
      }, session('s-1', '/work/a', 300)),
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
});

describe('分片隔离', () => {
  it('快照游标取较老水位，无基线时取快照水位', () => {
    expect(snapshotCursor(null, 7)).toBe(7);
    expect(snapshotCursor(9, 7)).toBe(7);
    expect(snapshotCursor(5, 9)).toBe(5);
  });

  it('重复到达的事实只应用一次', () => {
    const state = withTree(loaded(), nodeA, tree('s-1', 7, 1, [message(1, null, 'user')]));
    const event = fact(nodeA, 8, 108, { sessionId: 's-1', kind: 'session_renamed' }, session('s-1', '/work/a', 200, { title: '改名' }));

    const once = streamed(state, event);
    const twice = streamed(once, event);
    expect(twice).toBe(once);
    expect(shardOf(twice, nodeA).sessions[0]?.title).toBe('改名');
  });

  it('乱序到达的旧事实不改变已有状态', () => {
    const newer = streamed(
      loaded(),
      fact(nodeA, 8, 110, { sessionId: 's-1', kind: 'session_renamed' }, session('s-1', '/work/a', 300, { title: '新' })),
    );
    const older = streamed(
      newer,
      fact(nodeA, 9, 105, { sessionId: 's-1', kind: 'session_renamed' }, session('s-1', '/work/a', 200, { title: '旧' })),
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
        fact(nodeA, 8, 108, { sessionId: 's-2', kind: 'session_created' }, session('s-2', '/work/a', 200)),
      ),
      fact(nodeB, 9, 208, { sessionId: 's-3', kind: 'session_created' }, session('s-3', '/work/b', 400)),
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
      fact(nodeA, 6, 108, { sessionId: 's-1', kind: 'session_created' }, session('s-1', '/work/a', 108)),
    );

    expect(advanced.nodes[nodeA]).toBeUndefined();
    expect(advanced.hub.cursor).toBe(6);

    const bootstrapped = reduce(advanced, {
      type: 'node-bootstrap',
      node: nodeA,
      bootstrap: bootstrap([workspaceA], [session('s-1', '/work/a', 108)], 108),
    });
    expect(shardOf(bootstrapped, nodeA).sessions).toHaveLength(1);
    expect(shardOf(bootstrapped, nodeA).lastEventId).toBe(108);
  });

  it('广播序号重复的事件被忽略', () => {
    const state = loaded();
    const once = streamed(
      state,
      fact(nodeA, 8, 108, { sessionId: 's-1', kind: 'session_renamed' }, session('s-1', '/work/a', 200, { title: '新' })),
    );
    const replayed = streamed(
      once,
      fact(nodeB, 8, 208, { sessionId: 's-1', kind: 'session_renamed' }, session('s-1', '/work/b', 400, { title: '串写' })),
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
      node: {
        name: 'legacy-lab',
        url: 'http://127.0.0.1:7081',
        enabled: true,
        createdAt: 40,
        updatedAt: 40,
      },
    });
    expect(registered.hub.nodes.at(-1)).toEqual({
      name: 'legacy-lab',
      url: 'http://127.0.0.1:7081',
      enabled: true,
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
    expect(shardOf(bootstrapped, 'legacy-lab').sessions).toHaveLength(1);
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
      enabled: true,
      status: 'unreachable',
      detail: '连接节点失败: 隧道已断开',
    });
    expect(shardOf(state, nodeA).sessions).not.toEqual([]);
  });

  it('改名按删除加登记广播，旧名与旧分片一起消失', () => {
    const removed = streamed(loaded(), { type: 'node_removed', hubSeq: 8, name: nodeB });
    const registered = streamed(removed, {
      type: 'node_registered',
      hubSeq: 9,
      node: {
        name: 'studio-2',
        url: `http://127.0.0.1:${nodeB}`,
        enabled: true,
        createdAt: 20,
        updatedAt: 20,
      },
    });

    expect(registered.hub.nodes.map((record) => record.name)).toEqual([nodeA, 'studio-2']);
    expect(registered.hub.nodes.at(-1)?.status).toBe('unreachable');
    expect(registered.nodes[nodeB]).toBeUndefined();
    expect(registered.hub.cursor).toBe(9);
  });

  it('node_removed 删除记录与分片', () => {
    const removed = streamed(loaded(), { type: 'node_removed', hubSeq: 8, name: nodeB });

    expect(removed.hub.nodes.map((record) => record.name)).toEqual([nodeA]);
    expect(removed.nodes[nodeB]).toBeUndefined();
    expect(removed.hub.cursor).toBe(8);
  });

  it('node_updated 只改 URL 与启用标志，不动状态与分片', () => {
    const state = loaded();
    const updated = streamed(state, {
      type: 'node_updated',
      hubSeq: 8,
      node: {
        name: nodeB,
        url: 'http://127.0.0.1:19090',
        enabled: false,
        createdAt: 20,
        updatedAt: 30,
      },
    });

    expect(updated.hub.nodes.find((record) => record.name === nodeB)?.url).toBe(
      'http://127.0.0.1:19090',
    );
    expect(updated.hub.nodes.find((record) => record.name === nodeB)?.enabled).toBe(false);
    expect(updated.hub.nodes.find((record) => record.name === nodeB)?.status).toBe(
      state.hub.nodes.find((record) => record.name === nodeB)?.status,
    );
    expect(updated.nodes[nodeB]).toBe(state.nodes[nodeB]);
    expect(updated.hub.cursor).toBe(8);
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
          sessionId: 's-1',
          kind: 'message_appended',
          message: question,
        }, session('s-1', '/work/a', 108)),
      ),
      fact(nodeA, 9, 109, {
        sessionId: 's-1',
        kind: 'message_appended',
        message: answer,
      }, session('s-1', '/work/a', 109)),
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
      fact(nodeA, 8, 108, { sessionId: 's-1', kind: 'session_renamed' }, session('s-1', '/work/a', 108, { title: '新' })),
    );
    const landed = withTree(state, nodeA, tree('s-1', 400, null, []));

    expect(shardOf(landed, nodeA).lastEventId).toBe(108);
    expect(shardOf(landed, nodeA).sessions.map((entry) => entry.id)).toEqual(['s-1']);
  });

  it('一个节点的快照不改变另一节点的分片', () => {
    const state = streamed(
      loaded(),
      fact(nodeB, 8, 308, { sessionId: 's-1', kind: 'session_renamed' }, session('s-1', '/work/b', 308, { title: 'B 改名' })),
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
        sessionId: 's-1',
        kind: 'message_appended',
        message: question,
      }, session('s-1', '/work/a', 108)),
    );
    replayed = streamed(
      replayed,
      fact(nodeA, 9, 109, {
        sessionId: 's-1',
        kind: 'message_appended',
        message: answer,
      }, session('s-1', '/work/a', 109)),
    );
    replayed = streamed(
      replayed,
      fact(nodeA, 10, 110, {
        sessionId: 's-1',
        kind: 'active_leaf_changed',
        activeLeaf: 2,
      }, session('s-1', '/work/a', 110)),
    );

    const snapshot = withTree(base, nodeA, tree('s-1', 110, 2, [question, answer]));

    expect(transcriptIds(replayed, nodeA)).toEqual(transcriptIds(snapshot, nodeA));
  });

  it('加载快照后重放新事实等于从零构建', () => {
    const base = loaded();
    const append = fact(nodeA, 8, 108, {
      sessionId: 's-1',
      kind: 'message_appended',
      message: question,
    }, session('s-1', '/work/a', 108));
    const leaf = fact(nodeA, 9, 109, {
      sessionId: 's-1',
      kind: 'active_leaf_changed',
      activeLeaf: 1,
    }, session('s-1', '/work/a', 109));

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

  it('分片整体保留快照的协议字段，只取走信封字段', () => {
    type ExtendedTree = SessionTreeSnapshot & { futureField: string };
    const extended: ExtendedTree = { ...tree('s-2', 9, null, []), futureField: 'x' };

    const shard = loadSessionTree(shardOf(loaded(), nodeA), extended);

    expect(shard.trees['s-2']).toEqual({
      activeLeaf: null,
      nodes: [],
      queue: [],
      runs: [],
      futureField: 'x',
    });
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
    progress: null,
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
        sessionId: 's-1',
        kind: 'run_started',
        run: runningRun,
      }, session('s-1', '/work/a', 200, { busy: true })),
    );
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

  it('文本重新流动后清除等待', () => {
    const retrying = streamed(withRunning(), retryEvent(nodeA, 's-1'));
    const streaming = streamed(retrying, delta(nodeA, 's-1', 2, '恢复'));

    expect(shardOf(streaming, nodeA).retries['s-1']).toBeUndefined();
  });

  it('run 结束与 session 树重载都会清除重试等待', () => {
    const retrying = streamed(withRunning(), retryEvent(nodeA, 's-1'));
    const finished = streamed(
      retrying,
      fact(nodeA, 9, 109, {
        sessionId: 's-1',
        kind: 'run_finished',
        run: {
          ...runningRun,
          status: 'failed',
          failure: { code: 'model_rate_limited', message: '模型返回非成功状态 429: 限流' },
          updatedAt: 300,
        },
      }, session('s-1', '/work/a', 300)),
    );

    expect(shardOf(finished, nodeA).retries['s-1']).toBeUndefined();

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
