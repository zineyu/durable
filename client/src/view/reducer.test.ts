import { beforeEach, describe, expect, it } from 'vitest';

import type { BootstrapResponse } from '../api/generated/BootstrapResponse';
import type { FactEvent } from '../api/generated/FactEvent';
import type { MessageNode } from '../api/generated/MessageNode';
import type { RunRecord } from '../api/generated/RunRecord';
import type { SessionMetadata } from '../api/generated/SessionMetadata';
import type { StreamEvent } from '../api/generated/StreamEvent';
import type { WorkspaceMetadata } from '../api/generated/WorkspaceMetadata';
import { toSessionId } from './ids';
import {
  branchPosition,
  initialClientState,
  loadSessionTree,
  messageView,
  reduce,
  sidebarWorkspaces,
  transcriptNodes,
} from './reducer';
import type { ClientState } from './reducer';

const workspaceA: WorkspaceMetadata = { path: '/work/a', registeredAt: 10 };
const workspaceB: WorkspaceMetadata = { path: '/work/b', registeredAt: 20 };

let nextFactId = 100;

beforeEach(() => {
  nextFactId = 100;
});

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

function node(
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

function bootstrap(workspaces: WorkspaceMetadata[], sessions: SessionMetadata[]): BootstrapResponse {
  return { model: 'gpt-5-codex', maxEventId: 7, workspaces, sessions };
}

function fact(id: number, event: FactEvent): StreamEvent {
  return { type: 'fact', id, sessionId: null, timestamp: id, event };
}

function applied(state: ClientState, event: StreamEvent): ClientState {
  return reduce(state, { type: 'stream', event });
}

function applyFact(state: ClientState, event: FactEvent): ClientState {
  nextFactId += 1;
  return applied(state, fact(nextFactId, event));
}

function applyDelta(
  state: ClientState,
  sessionId: string,
  messageId: number,
  text: string,
): ClientState {
  return applied(state, { type: 'delta', sessionId, messageId, text, timestamp: 1 });
}

function withBootstrap(
  workspaces: WorkspaceMetadata[],
  sessions: SessionMetadata[],
): ClientState {
  return reduce(initialClientState, { type: 'bootstrap', bootstrap: bootstrap(workspaces, sessions) });
}

describe('侧栏状态', () => {
  it('按 workspace 注册顺序与 session 最近更新时间排序', () => {
    const state = withBootstrap(
      [workspaceB, workspaceA],
      [
        session('s-old', '/work/a', 100),
        session('s-new', '/work/a', 300),
        session('s-mid', '/work/a', 200),
        session('s-b', '/work/b', 50),
      ],
    );

    expect(state.modelName).toBe('gpt-5-codex');
    expect(state.lastEventId).toBe(7);
    expect(sidebarWorkspaces(state)).toEqual([
      {
        id: '/work/a',
        path: '/work/a',
        sessions: [
          { id: toSessionId('s-new'), title: '标题 s-new', state: 'idle', queuedCount: 0 },
          { id: toSessionId('s-mid'), title: '标题 s-mid', state: 'idle', queuedCount: 0 },
          { id: toSessionId('s-old'), title: '标题 s-old', state: 'idle', queuedCount: 0 },
        ],
      },
      {
        id: '/work/b',
        path: '/work/b',
        sessions: [{ id: toSessionId('s-b'), title: '标题 s-b', state: 'idle', queuedCount: 0 }],
      },
    ]);
  });

  it('由 busy 与队列长度推导忙碌状态', () => {
    const state = withBootstrap(
      [workspaceA],
      [
        session('s-running', '/work/a', 100, { busy: true }),
        session('s-queued', '/work/a', 200, { queueLength: 2 }),
      ],
    );

    expect(sidebarWorkspaces(state)[0]?.sessions).toEqual([
      { id: toSessionId('s-queued'), title: '标题 s-queued', state: 'queued', queuedCount: 2 },
      { id: toSessionId('s-running'), title: '标题 s-running', state: 'running', queuedCount: 0 },
    ]);
  });

  it('注册与移除 workspace 同步增删其 session', () => {
    const state = withBootstrap([workspaceB], [session('s-b', '/work/b', 100)]);

    const registered = applyFact(state, { kind: 'workspace_registered', workspace: workspaceA });
    expect(sidebarWorkspaces(registered).map((workspace) => workspace.path)).toEqual([
      '/work/a',
      '/work/b',
    ]);

    const withSession = applyFact(registered, {
      kind: 'session_created',
      session: session('s-a', '/work/a', 200),
    });
    const removed = applyFact(withSession, { kind: 'workspace_removed', path: '/work/a' });
    expect(sidebarWorkspaces(removed).map((workspace) => workspace.path)).toEqual(['/work/b']);
  });

  it('重命名与删除 session', () => {
    const state = withBootstrap(
      [workspaceA],
      [session('s-1', '/work/a', 200), session('s-2', '/work/a', 100)],
    );

    const renamed = applyFact(state, {
      kind: 'session_renamed',
      session: session('s-2', '/work/a', 300, { title: '新标题' }),
    });
    expect(sidebarWorkspaces(renamed)[0]?.sessions[0]).toEqual({
      id: toSessionId('s-2'),
      title: '新标题',
      state: 'idle',
      queuedCount: 0,
    });

    const deleted = applyFact(renamed, { kind: 'session_deleted', sessionId: 's-2' });
    expect(sidebarWorkspaces(deleted)[0]?.sessions).toEqual([
      { id: toSessionId('s-1'), title: '标题 s-1', state: 'idle', queuedCount: 0 },
    ]);
  });

  it('移除队列项后减少队列长度与队列视图', () => {
    const state = loadSessionTree(withBootstrap([workspaceA], [session('s-1', '/work/a', 100)]), {
      session: session('s-1', '/work/a', 100, { queueLength: 1 }),
      activeLeaf: null,
      nodes: [],
      queue: [{ id: 5, text: '排队', anchor: null, createdAt: 5 }],
      runs: [],
    });

    const next = applyFact(state, { kind: 'queue_item_removed', sessionId: 's-1', itemId: 5 });
    expect(next.sessions[0]?.queueLength).toBe(0);
    expect(next.trees['s-1']?.queue).toEqual([]);
  });
});

describe('transcript 推导', () => {
  const base = (nodes: MessageNode[], activeLeaf: number | null): ClientState =>
    loadSessionTree(withBootstrap([workspaceA], [session('s-1', '/work/a', 100)]), {
      session: session('s-1', '/work/a', 100),
      activeLeaf,
      nodes,
      queue: [],
      runs: [],
    });

  it('取 root 到 active leaf 的路径', () => {
    const nodes = [
      node(1, null, 'user', { text: '一' }),
      node(2, 1, 'assistant', { text: '二' }),
      node(3, 2, 'user', { text: '三' }),
      node(4, 3, 'assistant', { text: '四' }),
    ];
    expect(transcriptNodes(base(nodes, 4), 's-1').map((entry) => entry.id)).toEqual([1, 2, 3, 4]);
    expect(transcriptNodes(base(nodes, 2), 's-1').map((entry) => entry.id)).toEqual([1, 2]);
    expect(transcriptNodes(base(nodes, null), 's-1')).toEqual([]);
  });

  it('切换 active leaf 后 transcript 跟随目标子树', () => {
    const nodes = [
      node(1, null, 'user', { text: '原始' }),
      node(2, 1, 'assistant', { text: '原始回答' }),
      node(3, 1, 'user', { text: '编辑' }),
      node(4, 3, 'assistant', { text: '编辑回答' }),
    ];
    const edited = base(nodes, 4);
    expect(transcriptNodes(edited, 's-1').map((entry) => entry.id)).toEqual([1, 3, 4]);

    const switched = applyFact(edited, {
      kind: 'active_leaf_changed',
      session: session('s-1', '/work/a', 200),
      activeLeaf: 2,
    });
    expect(transcriptNodes(switched, 's-1').map((entry) => entry.id)).toEqual([1, 2]);
  });

  it('分支位置给出兄弟序号与相邻节点', () => {
    const nodes = [
      node(1, null, 'user', { text: '第一问' }),
      node(2, 1, 'assistant', { text: '第一答' }),
      node(3, 2, 'user', { text: '第二问' }),
      node(4, 3, 'assistant', { text: '第二答' }),
      node(5, 2, 'user', { text: '第二问改写' }),
      node(6, 5, 'assistant', { text: '改写后的回答' }),
    ];
    const state = base(nodes, 6);
    const edited = nodes[4];
    const root = nodes[0];
    if (edited === undefined || root === undefined) {
      throw new Error('缺少测试节点');
    }
    expect(transcriptNodes(state, 's-1').map((entry) => entry.id)).toEqual([1, 2, 5, 6]);
    expect(branchPosition(state, 's-1', edited)).toEqual({
      index: 2,
      total: 2,
      previousId: 3,
      nextId: null,
    });
    expect(branchPosition(state, 's-1', root)).toBeNull();
  });
});

describe('流式增量与 run 状态', () => {
  const withAssistant = (): ClientState =>
    loadSessionTree(withBootstrap([workspaceA], [session('s-1', '/work/a', 100)]), {
      session: session('s-1', '/work/a', 100),
      activeLeaf: 2,
      nodes: [node(1, null, 'user', { text: '问' }), node(2, 1, 'assistant', { runId: 1 })],
      queue: [],
      runs: [],
    });

  it('同一 run 的多个 delta 按序合并为连续文本', () => {
    let state = withAssistant();
    state = applyDelta(state, 's-1', 2, '你');
    state = applyDelta(state, 's-1', 2, '好');
    expect(state.trees['s-1']?.nodes[1]?.text).toBe('你好');
  });

  it('run 结束事件以完整文本覆盖增量', () => {
    let state = withAssistant();
    state = applyDelta(state, 's-1', 2, '部分');
    state = applyFact(state, {
      kind: 'message_updated',
      session: session('s-1', '/work/a', 300),
      message: node(2, 1, 'assistant', { text: '完整文本', runId: 1 }),
    });
    expect(state.trees['s-1']?.nodes[1]?.text).toBe('完整文本');
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
    let state = withAssistant();
    state = applyFact(state, {
      kind: 'run_started',
      session: session('s-1', '/work/a', 200, { busy: true }),
      run,
    });
    const assistant = state.trees['s-1']?.nodes[1];
    if (assistant === undefined) {
      throw new Error('缺少 assistant 节点');
    }
    expect(messageView(state, 's-1', assistant)).toEqual({ state: 'streaming', note: null });

    state = applyFact(state, {
      kind: 'run_finished',
      session: session('s-1', '/work/a', 300),
      run: {
        ...run,
        status: 'failed',
        failure: { code: 'model_upstream', message: '上游 502' },
        updatedAt: 300,
      },
    });
    expect(messageView(state, 's-1', assistant)).toEqual({ state: 'failed', note: '上游 502' });
  });
});

describe('事件流的幂等与隔离', () => {
  it('重复事件被忽略', () => {
    const state = loadSessionTree(withBootstrap([workspaceA], [session('s-1', '/work/a', 100)]), {
      session: session('s-1', '/work/a', 100),
      activeLeaf: 1,
      nodes: [node(1, null, 'user', { text: '一' })],
      queue: [],
      runs: [],
    });

    const event = fact(9, {
      kind: 'session_renamed',
      session: session('s-1', '/work/a', 200, { title: '改名' }),
    });
    const once = applied(state, event);
    const twice = applied(once, event);
    expect(twice).toBe(once);
    expect(twice.sessions[0]?.title).toBe('改名');
  });

  it('乱序到达的旧事件不改变已有状态', () => {
    const state = withBootstrap([workspaceA], [session('s-1', '/work/a', 100)]);

    const newer = applied(
      state,
      fact(10, {
        kind: 'session_renamed',
        session: session('s-1', '/work/a', 300, { title: '新' }),
      }),
    );
    const older = applied(
      newer,
      fact(8, {
        kind: 'session_renamed',
        session: session('s-1', '/work/a', 200, { title: '旧' }),
      }),
    );
    expect(older).toBe(newer);
    expect(older.sessions[0]?.title).toBe('新');
  });

  it('两个 session 的事件交错到达时互不影响', () => {
    const state = withBootstrap(
      [workspaceA],
      [session('s-1', '/work/a', 100), session('s-2', '/work/a', 100)],
    );
    const withTrees = loadSessionTree(
      loadSessionTree(state, {
        session: session('s-1', '/work/a', 100),
        activeLeaf: 2,
        nodes: [node(1, null, 'user', { text: 'A 问' }), node(2, 1, 'assistant', { runId: 1 })],
        queue: [],
        runs: [],
      }),
      {
        session: session('s-2', '/work/a', 100),
        activeLeaf: 5,
        nodes: [node(5, null, 'assistant', { runId: 1 })],
        queue: [],
        runs: [],
      },
    );

    const interleaved = applyDelta(applyDelta(withTrees, 's-2', 5, 'B 输出'), 's-1', 2, 'A 输出');

    expect(interleaved.trees['s-2']?.nodes[0]?.text).toBe('B 输出');
    expect(interleaved.trees['s-1']?.nodes[1]?.text).toBe('A 输出');
  });

  it('回放事件构建的 transcript 等于 tree 快照', () => {
    const base = withBootstrap([workspaceA], [session('s-1', '/work/a', 100)]);

    let replayed = applyFact(base, {
      kind: 'message_appended',
      session: session('s-1', '/work/a', 110),
      message: node(1, null, 'user', { text: '问' }),
    });
    replayed = applyFact(replayed, {
      kind: 'message_appended',
      session: session('s-1', '/work/a', 120),
      message: node(2, 1, 'assistant', { text: '答', runId: 1 }),
    });
    replayed = applyFact(replayed, {
      kind: 'active_leaf_changed',
      session: session('s-1', '/work/a', 130),
      activeLeaf: 2,
    });

    const snapshot = loadSessionTree(base, {
      session: session('s-1', '/work/a', 130),
      activeLeaf: 2,
      nodes: [
        node(1, null, 'user', { text: '问' }),
        node(2, 1, 'assistant', { text: '答', runId: 1 }),
      ],
      queue: [],
      runs: [],
    });

    expect(transcriptNodes(replayed, 's-1')).toEqual(transcriptNodes(snapshot, 's-1'));
  });
});

describe('连接状态', () => {
  it('记录连接状态与错误横幅', () => {
    const connected = reduce(initialClientState, { type: 'connection', connection: 'connected' });
    expect(connected.connection).toBe('connected');

    const failed = reduce(connected, {
      type: 'error',
      error: { code: 'run_in_progress', message: 'run 正在执行' },
    });
    expect(failed.error).toEqual({ code: 'run_in_progress', message: 'run 正在执行' });

    expect(reduce(failed, { type: 'dismiss-error' }).error).toBeNull();
  });
});
