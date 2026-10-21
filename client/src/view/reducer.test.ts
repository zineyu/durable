import { describe, expect, it } from 'vitest';

import type { BootstrapResponse } from '../api/generated/BootstrapResponse';
import type { FactEvent } from '../api/generated/FactEvent';
import type { HubBootstrap } from '../api/generated/HubBootstrap';
import type { HubStreamEvent } from '../api/generated/HubStreamEvent';
import type { MessageNode } from '../api/generated/MessageNode';
import type { NodeSnapshot } from '../api/generated/NodeSnapshot';
import type { NodeStatus } from '../api/generated/NodeStatus';
import type { RunRecord } from '../api/generated/RunRecord';
import type { SessionMetadata } from '../api/generated/SessionMetadata';
import type { SessionTree } from '../api/generated/SessionTree';
import type { ToolCallRecord } from '../api/generated/ToolCallRecord';
import type { WorkspaceMetadata } from '../api/generated/WorkspaceMetadata';
import { toSessionId } from './ids';
import {
  branchPosition,
  loadSessionTree,
  messageView,
  sessionQueue,
  sidebarWorkspaces,
  snapshotCursor,
  transcriptNodes,
} from './node-state';
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
  role: MessageNode['role'],
  extra: Partial<MessageNode> = {},
): MessageNode {
  return {
    id,
    parentId,
    role,
    text: '',
    runId: null,
    toolCalls: [],
    createdAt: id,
    ...extra,
  };
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
  text: string,
): HubStreamEvent {
  return { type: 'delta', node, sessionId: toSessionId(sessionId), messageId, text, timestamp: 1 };
}

function at(nodes: MessageNode[], index: number): MessageNode {
  const node = nodes[index];
  if (node === undefined) {
    throw new Error('缺少测试节点');
  }
  return node;
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
  nodes: MessageNode[],
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
          },
          {
            id: toSessionId('s-mid'),
            title: '标题 s-mid',
            state: 'idle',
            queuedCount: 0,
            updatedAt: 200,
          },
          {
            id: toSessionId('s-old'),
            title: '标题 s-old',
            state: 'idle',
            queuedCount: 0,
            updatedAt: 100,
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
      },
      {
        id: toSessionId('s-running'),
        title: '标题 s-running',
        state: 'running',
        queuedCount: 0,
        updatedAt: 100,
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
    const state = withTree(
      loaded(),
      nodeA,
      tree('s-1', 7, 1, [message(1, null, 'user', { text: '问' })]),
    );

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
      queue: [{ id: 5, text: '排队', anchor: null, createdAt: 5 }],
    });

    const next = streamed(
      state,
      fact(nodeA, 8, 108, { kind: 'queue_item_removed', sessionId: 's-1', itemId: 5 }),
    );
    expect(shardOf(next, nodeA).sessions[0]?.queueLength).toBe(0);
    expect(sessionQueue(shardOf(next, nodeA), 's-1')).toEqual([]);
  });
});

describe('transcript 推导', () => {
  const nodes = [
    message(1, null, 'user', { text: '一' }),
    message(2, 1, 'assistant', { text: '二' }),
    message(3, 2, 'user', { text: '三' }),
    message(4, 3, 'assistant', { text: '四' }),
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
      message(1, null, 'user', { text: '原始' }),
      message(2, 1, 'assistant', { text: '原始回答' }),
      message(3, 1, 'user', { text: '编辑' }),
      message(4, 3, 'assistant', { text: '编辑回答' }),
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

  it('分支位置给出兄弟序号与相邻节点', () => {
    const branched = [
      message(1, null, 'user', { text: '第一问' }),
      message(2, 1, 'assistant', { text: '第一答' }),
      message(3, 2, 'user', { text: '第二问' }),
      message(4, 3, 'assistant', { text: '第二答' }),
      message(5, 2, 'user', { text: '第二问改写' }),
      message(6, 5, 'assistant', { text: '改写后的回答' }),
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
});

describe('流式增量与 run 状态', () => {
  const withAssistant = (): ClientState =>
    withTree(loaded(), nodeA, tree('s-1', 7, 2, [
      message(1, null, 'user', { text: '问' }),
      message(2, 1, 'assistant', { runId: 1 }),
    ]));

  it('同一 run 的多个 delta 按序合并为连续文本', () => {
    let state = withAssistant();
    state = streamed(state, delta(nodeA, 's-1', 2, '你'));
    state = streamed(state, delta(nodeA, 's-1', 2, '好'));
    expect(shardOf(state, nodeA).trees['s-1']?.nodes[1]?.text).toBe('你好');
  });

  it('delta 不推进广播游标', () => {
    const state = streamed(withAssistant(), delta(nodeA, 's-1', 2, '你'));
    expect(state.hub.cursor).toBe(7);
  });

  it('工具调用事实事件在 run 进行中追加到 assistant 节点', () => {
    const record: ToolCallRecord = {
      id: 'call_1',
      name: 'read_file',
      arguments: '{"path":"devenv.nix"}',
      outcome: { status: 'succeeded', content: 'scripts.build.exec = "durable-build"' },
    };
    let state = withAssistant();
    state = streamed(
      state,
      fact(nodeA, 8, 108, {
        kind: 'tool_call_recorded',
        session: session('s-1', '/work/a', 200, { busy: true }),
        message: 2,
        toolCall: record,
      }),
    );
    state = streamed(
      state,
      fact(nodeA, 9, 109, {
        kind: 'tool_call_recorded',
        session: session('s-1', '/work/a', 200, { busy: true }),
        message: 2,
        toolCall: record,
      }),
    );
    expect(shardOf(state, nodeA).trees['s-1']?.nodes[1]?.toolCalls).toEqual([record]);
  });

  it('run 结束事件以完整文本覆盖增量', () => {
    let state = streamed(withAssistant(), delta(nodeA, 's-1', 2, '部分'));
    state = streamed(
      state,
      fact(nodeA, 8, 108, {
        kind: 'message_updated',
        session: session('s-1', '/work/a', 300),
        message: message(2, 1, 'assistant', { text: '完整文本', runId: 1 }),
      }),
    );
    expect(shardOf(state, nodeA).trees['s-1']?.nodes[1]?.text).toBe('完整文本');
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
      withAssistant(),
      fact(nodeA, 8, 108, {
        kind: 'run_started',
        session: session('s-1', '/work/a', 200, { busy: true }),
        run,
      }),
    );
    const assistant = shardOf(state, nodeA).trees['s-1']?.nodes[1];
    if (assistant === undefined) {
      throw new Error('缺少 assistant 节点');
    }
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
    const state = withTree(loaded(), nodeA, tree('s-1', 7, 1, [message(1, null, 'user', { text: '一' })]));
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

  it('两节点同名 session 的增量互不串写', () => {
    let state = withTree(
      withTree(loaded(), nodeA, tree('s-1', 7, 2, [
        message(1, null, 'user', { text: 'A 问' }),
        message(2, 1, 'assistant', { runId: 1 }),
      ])),
      nodeB,
      tree('s-1', 7, 5, [message(5, null, 'assistant', { runId: 1 })]),
    );

    state = streamed(state, delta(nodeB, 's-1', 5, 'B 输出'));
    state = streamed(state, delta(nodeA, 's-1', 2, 'A 输出'));

    expect(shardOf(state, nodeB).trees['s-1']?.nodes[0]?.text).toBe('B 输出');
    expect(shardOf(state, nodeA).trees['s-1']?.nodes[1]?.text).toBe('A 输出');
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
  const question = message(1, null, 'user', { text: '问' });
  const answer = message(2, 1, 'assistant', { text: '答', runId: 1 });

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

describe('工具调用', () => {
  it('工具调用与结果随 assistant 消息进入 transcript', () => {
    const record: ToolCallRecord = {
      id: 'call_1',
      name: 'bash',
      arguments: '{"command":"cargo test"}',
      outcome: { status: 'succeeded', content: '测试通过' },
    };
    let state = streamed(
      loaded(),
      fact(nodeA, 8, 108, {
        kind: 'message_appended',
        session: session('s-1', '/work/a', 108),
        message: message(1, null, 'user', { text: '执行' }),
      }),
    );
    state = streamed(
      state,
      fact(nodeA, 9, 109, {
        kind: 'message_updated',
        session: session('s-1', '/work/a', 109),
        message: message(2, 1, 'assistant', { text: '完成', runId: 1, toolCalls: [record] }),
      }),
    );
    state = streamed(
      state,
      fact(nodeA, 10, 110, {
        kind: 'active_leaf_changed',
        session: session('s-1', '/work/a', 110),
        activeLeaf: 2,
      }),
    );

    expect(transcriptNodes(shardOf(state, nodeA), 's-1')[1]?.toolCalls).toEqual([record]);
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
    const state = withTree(
      loaded(),
      nodeA,
      tree('s-1', 7, 2, [
        message(1, null, 'user', { text: '问' }),
        message(2, 1, 'assistant', { runId: 1 }),
      ]),
    );
    return streamed(
      state,
      fact(nodeA, 8, 108, {
        kind: 'run_started',
        session: session('s-1', '/work/a', 200, { busy: true }),
        run: runningRun,
      }),
    );
  }

  function assistantOf(state: ClientState): MessageNode {
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
        message(1, null, 'user', { text: '问' }),
        message(2, 1, 'assistant', { runId: 1 }),
      ]),
    );
    expect(shardOf(reloaded, nodeA).retries['s-1']).toBeUndefined();
  });
});
