import { describe, expect, it } from 'vitest';

import type { ContentPart } from '../api/generated/ContentPart';
import type { MessageSkeleton } from '../api/generated/MessageSkeleton';
import type { RunRecord } from '../api/generated/RunRecord';
import type { SessionMetadata } from '../api/generated/SessionMetadata';
import type { WorkspaceMetadata } from '../api/generated/WorkspaceMetadata';
import {
  missingBodyIds,
  missingShards,
  nodeSummary,
  orderedSessions,
  paletteSessions,
  hasSession,
  runningEntries,
  sessionSummaries,
  sessionView,
  workspaceSummaries,
} from './derived';
import { toQueueItemId, toSessionId } from './ids';
import { emptyNodeState } from './node-state';
import type { NodeState, SessionTreeState } from './node-state';
import type { NodeRecord, SessionView, TranscriptEntry } from './types';

/// 派生层用例直接构造分片，不经过 reducer。

const workspaceA: WorkspaceMetadata = { path: '/work/a', registeredAt: 10 };
const workspaceB: WorkspaceMetadata = { path: '/work/b', registeredAt: 20 };

const nodeA: NodeRecord = {
  name: 'macbook-air-01',
  url: 'http://a',
  enabled: true,
  status: 'ok',
  detail: null,
};
const nodeB: NodeRecord = {
  name: 'studio',
  url: 'http://b',
  enabled: true,
  status: 'ok',
  detail: null,
};

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
  return { id, parentId, role, runId: null, createdAt: id, parts: [], ...extra };
}

function run(
  id: number,
  status: RunRecord['status'],
  extra: Partial<RunRecord> = {},
): RunRecord {
  return {
    id,
    sessionId: 's-1',
    status,
    failure: null,
    createdAt: id,
    updatedAt: id,
    ...extra,
  };
}

function text(value: string): ContentPart[] {
  return [{ kind: 'text', text: value, providerMeta: null }];
}

function tree(
  nodes: MessageSkeleton[],
  activeLeaf: number | null,
  extra: Partial<SessionTreeState> = {},
): SessionTreeState {
  return { activeLeaf, nodes, queue: [], runs: [], ...extra };
}

/// 默认带一个 session `s-1`，使 `sessionView(分片, 's-1')` 有对象可查。
function shard(overrides: Partial<NodeState> = {}): NodeState {
  return {
    ...emptyNodeState,
    sessions: [session('s-1', '/work/a', 10)],
    ...overrides,
  };
}

function viewOf(value: NodeState, sessionId = 's-1'): SessionView {
  const view = sessionView(value, sessionId);
  if (view === null) {
    throw new Error(`缺少 session 视图: ${sessionId}`);
  }
  return view;
}

function entryOf(view: SessionView, id: number): TranscriptEntry {
  const entry = view.transcript.find((candidate) => candidate.message.id === id);
  if (entry === undefined) {
    throw new Error(`transcript 里没有消息 ${id}`);
  }
  return entry;
}

describe('session 摘要', () => {
  it('状态由忙碌标志与队列长度推导', () => {
    const value = shard({
      sessions: [
        session('s-running', '/work/a', 100, { busy: true }),
        session('s-queued', '/work/a', 200, { queueLength: 2 }),
        session('s-idle', '/work/a', 300),
      ],
    });

    expect(sessionSummaries(value).map((summary) => summary.state)).toEqual([
      'running',
      'queued',
      'idle',
    ]);
    expect(sessionSummaries(value).map((summary) => summary.queuedCount)).toEqual([0, 2, 0]);
  });

  it('未读取已查看时间，结局取最近一次 run', () => {
    const value = shard({
      sessions: [session('s-1', '/work/a', 30), session('s-2', '/work/a', 10)],
      lastSeenAt: { 's-1': 20, 's-2': 10 },
      trees: {
        's-1': tree([], null, {
          runs: [
            run(1, 'failed', { updatedAt: 4 }),
            run(2, 'succeeded', { updatedAt: 8 }),
            run(3, 'cancelled', { updatedAt: 6 }),
          ],
        }),
      },
    });

    const summaries = sessionSummaries(value);
    expect(summaries[0]?.unread).toBe(true);
    expect(summaries[0]?.outcome).toBe('succeeded');
    expect(summaries[1]?.unread).toBe(false);
    expect(summaries[1]?.outcome).toBeNull();
  });

  it('按 workspace 注册顺序与 session 最近更新时间分组', () => {
    const value = shard({
      workspaces: [workspaceB, workspaceA],
      sessions: [
        session('s-old', '/work/a', 100),
        session('s-new', '/work/a', 300),
        session('s-mid', '/work/a', 200),
        session('s-b', '/work/b', 50),
      ],
    });

    expect(workspaceSummaries(value).map((workspace) => workspace.path)).toEqual([
      '/work/a',
      '/work/b',
    ]);
    expect(workspaceSummaries(value)[0]?.sessions.map((summary) => summary.id)).toEqual([
      toSessionId('s-new'),
      toSessionId('s-mid'),
      toSessionId('s-old'),
    ]);
    expect(workspaceSummaries(value)[1]?.sessions.map((summary) => summary.id)).toEqual([
      toSessionId('s-b'),
    ]);
  });

  it('节点摘要带模型名，快捷键顺序沿用侧栏顺序', () => {
    const value = shard({
      modelName: 'gpt-5-codex',
      workspaces: [workspaceA, workspaceB],
      sessions: [session('s-a', '/work/a', 100), session('s-b', '/work/b', 200)],
    });

    const summary = nodeSummary(value);
    expect(summary.modelName).toBe('gpt-5-codex');
    expect(orderedSessions(summary.workspaces).map((entry) => entry.id)).toEqual([
      toSessionId('s-a'),
      toSessionId('s-b'),
    ]);
  });
});

describe('session 派生视图', () => {
  it('transcript 取 root 到 active leaf 的路径，不含工具节点', () => {
    const nodes = [
      message(1, null, 'user'),
      message(2, 1, 'assistant'),
      message(3, 2, 'tool'),
      message(4, 2, 'user'),
    ];

    expect(viewOf(shard({ trees: { 's-1': tree(nodes, 4) } })).transcript.map(
      (entry) => entry.message.id,
    )).toEqual([1, 2, 4]);
    expect(viewOf(shard({ trees: { 's-1': tree(nodes, 2) } })).transcript.map(
      (entry) => entry.message.id,
    )).toEqual([1, 2]);
    expect(viewOf(shard({ trees: { 's-1': tree(nodes, null) } })).transcript).toEqual([]);
    expect(sessionView(shard(), 'missing')).toBeNull();
  });

  it('分支序号取兄弟按 id 升序的位置，压缩节点不算兄弟', () => {
    const nodes = [
      message(1, null, 'user'),
      message(2, 1, 'assistant'),
      message(3, 2, 'user'),
      message(4, 3, 'assistant'),
      message(5, 2, 'user'),
      message(6, 5, 'assistant'),
      message(7, 2, 'compaction'),
    ];
    const view = viewOf(shard({ trees: { 's-1': tree(nodes, 6) } }));

    expect(view.transcript.map((entry) => entry.message.id)).toEqual([1, 2, 5, 6]);
    expect(entryOf(view, 5).branch).toEqual({
      index: 2,
      total: 2,
      previousId: 3,
      nextId: null,
    });
    expect(entryOf(view, 2).branch).toBeNull();
    expect(entryOf(view, 1).branch).toBeNull();
  });

  it('工具结局按 call id 汇总，正文未到时没有条目', () => {
    const nodes = [
      message(1, null, 'user'),
      message(2, 1, 'assistant'),
      message(3, 2, 'tool'),
    ];
    const unloaded = viewOf(shard({ trees: { 's-1': tree(nodes, 3) } }));
    expect(unloaded.toolResults).toEqual({});

    const loaded = viewOf(
      shard({
        trees: { 's-1': tree(nodes, 3) },
        bodies: {
          3: [
            {
              kind: 'tool_result',
              callId: 'call_1',
              state: { status: 'unknown' },
              providerMeta: null,
            },
          ],
        },
      }),
    );
    expect(loaded.toolResults).toEqual({ call_1: { status: 'unknown' } });
  });

  it('正文未到时 parts 为空，已拉到的正文原样给出', () => {
    const nodes = [message(1, null, 'user'), message(2, 1, 'assistant')];
    const view = viewOf(
      shard({ trees: { 's-1': tree(nodes, 2) }, bodies: { 2: text('回答') } }),
    );

    expect(entryOf(view, 1).parts).toBeNull();
    expect(entryOf(view, 2).parts).toEqual(text('回答'));
  });

  it('由 run 状态推导消息状态与说明', () => {
    const nodes = [message(1, null, 'user'), message(2, 1, 'assistant', { runId: 1 })];
    const running = (extra: Partial<NodeState>) =>
      viewOf(
        shard({
          trees: { 's-1': tree(nodes, 2, { runs: [run(1, 'running')] }) },
          ...extra,
        }),
      );

    expect(entryOf(running({}), 2).state).toBe('streaming');
    expect(entryOf(running({}), 2).note).toBeNull();
    expect(
      entryOf(running({ retries: { 's-1': { attempt: 1, maxAttempts: 3, delayMs: 2000 } } }), 2).note,
    ).toBe('供应商限流，第 1/3 次重试，等待 2 秒');

    const failed = viewOf(
      shard({
        trees: {
          's-1': tree(nodes, 2, {
            runs: [run(1, 'failed', { failure: { code: 'model_upstream', message: '上游 502' } })],
          }),
        },
      }),
    );
    expect(entryOf(failed, 2)).toMatchObject({
      state: 'failed',
      note: '模型上游错误：上游 502',
    });

    const cancelled = viewOf(
      shard({ trees: { 's-1': tree(nodes, 2, { runs: [run(1, 'cancelled')] }) } }),
    );
    expect(entryOf(cancelled, 2)).toMatchObject({ state: 'cancelled', note: null });
    expect(entryOf(cancelled, 1)).toMatchObject({ state: 'complete', note: null });
    expect(cancelled.lastRun).toBe('cancelled');
  });

  it('队列与占位随视图给出，空闲判定同时看 run 与队列', () => {
    const nodes = [message(1, null, 'user')];
    const value = shard({
      sessions: [session('s-1', '/work/a', 30, { busy: true, queueLength: 1 })],
      trees: {
        's-1': tree(nodes, 1, {
          queue: [{ id: 5, text: '排队', anchor: null, createdAt: 5, pendingSteer: true }],
        }),
      },
      placeholders: { 's-1': [{ id: 9, parts: text('部分') }] },
    });
    const view = viewOf(value);

    expect(view.queue).toEqual([
      { id: toQueueItemId('5'), text: '排队', pendingSteer: true },
    ]);
    expect(view.placeholders).toEqual([{ id: 9, parts: text('部分') }]);
    expect(view.idle).toBe(false);
    expect(viewOf(shard({ sessions: [session('s-1', '/work/a', 30)] })).idle).toBe(true);
  });

  it('已发送的用户正文按链上顺序成为输入框历史', () => {
    const nodes = [
      message(1, null, 'user'),
      message(2, 1, 'assistant'),
      message(3, 2, 'user'),
      message(4, 3, 'user'),
    ];
    const view = viewOf(
      shard({
        trees: { 's-1': tree(nodes, 3) },
        bodies: { 1: text('第一条'), 2: text('回答'), 3: text('   ') },
      }),
    );

    expect(view.history).toEqual(['第一条']);
    expect(viewOf(shard({ trees: { 's-1': tree(nodes, 4) } })).history).toEqual([]);
  });

  it('主面板所需的顺序、分支、工具结局与占位来自同一份派生', () => {
    const nodes = [
      message(1, null, 'user'),
      message(2, 1, 'assistant', { runId: 1 }),
      message(3, 2, 'tool'),
      message(4, 1, 'user'),
      message(5, 4, 'assistant'),
    ];
    const view = viewOf(
      shard({
        trees: {
          's-1': tree(nodes, 5, {
            runs: [run(1, 'succeeded')],
            queue: [{ id: 7, text: '排队', anchor: null, createdAt: 7, pendingSteer: false }],
          }),
        },
        bodies: {
          1: text('先做迁移。'),
          5: text('迁移已完成。'),
          3: [
            {
              kind: 'tool_result',
              callId: 'call_1',
              state: { status: 'finished', outcome: { status: 'succeeded', content: 'ok' } },
              providerMeta: null,
            },
          ],
        },
        placeholders: { 's-1': [{ id: 9, parts: text('新回答') }] },
      }),
    );

    expect(view.transcript.map((entry) => entry.message.id)).toEqual([1, 4, 5]);
    expect(entryOf(view, 4).branch).toEqual({
      index: 2,
      total: 2,
      previousId: 2,
      nextId: null,
    });
    expect(view.toolResults).toEqual({
      call_1: { status: 'finished', outcome: { status: 'succeeded', content: 'ok' } },
    });
    expect(view.placeholders.map((placeholder) => placeholder.id)).toEqual([9]);
    expect(view.queue.map((item) => item.id)).toEqual([toQueueItemId('7')]);
  });
});

describe('缺失正文', () => {
  it('只列出既无缓存也不在途、也没有失败的消息', () => {
    const nodes = [message(1, null, 'user'), message(2, 1, 'tool'), message(3, 1, 'user')];
    const value = shard({
      trees: { 's-1': tree(nodes, 3) },
      bodies: { 1: [] },
      bodyRequests: [2],
      bodyFailures: [],
    });

    expect(missingBodyIds(value, 's-1')).toEqual([3]);
    expect(missingBodyIds(value, 'missing')).toEqual([]);
  });
});

describe('session 存在判定', () => {
  it('分片里有该 session 才算存在', () => {
    expect(hasSession(shard(), 's-1')).toBe(true);
    expect(hasSession(shard(), 'missing')).toBe(false);
    expect(hasSession(shard({ sessions: [] }), 's-1')).toBe(false);
  });
});

describe('跨节点列表', () => {
  it('运行中面板跨节点列出运行中的 session 与队列长度', () => {
    const entries = runningEntries([nodeA, nodeB], {
      'macbook-air-01': shard({
        sessions: [session('s-1', '/work/a', 10, { busy: true }), session('s-2', '/work/a', 9)],
        trees: { 's-1': tree([], null, { runs: [run(3, 'running')] }) },
      }),
      studio: shard({
        sessions: [session('s-3', '/work/a', 12, { busy: true, queueLength: 2 })],
      }),
    });

    expect(entries).toEqual([
      {
        node: 'macbook-air-01',
        sessionId: toSessionId('s-1'),
        title: '标题 s-1',
        queueLength: 0,
        startedAt: 3,
      },
      {
        node: 'studio',
        sessionId: toSessionId('s-3'),
        title: '标题 s-3',
        queueLength: 2,
        startedAt: null,
      },
    ]);
  });

  it('run 结束或失败后不再出现在运行中面板', () => {
    const value = shard({
      sessions: [session('s-1', '/work/a', 10)],
      trees: {
        's-1': tree([], null, {
          runs: [run(1, 'running', { updatedAt: 5 }), run(1, 'succeeded', { updatedAt: 6 })],
        }),
      },
    });
    expect(runningEntries([nodeA], { 'macbook-air-01': value })).toEqual([]);
  });

  it('命令面板列出全部节点的 session 与来源节点', () => {
    const sessions = paletteSessions([nodeA, nodeB], {
      'macbook-air-01': shard({ sessions: [session('s-1', '/work/a', 20)] }),
      studio: shard({ sessions: [session('s-2', '/work/a', 30)] }),
    });

    expect(sessions).toEqual([
      {
        node: 'macbook-air-01',
        sessionId: toSessionId('s-1'),
        title: '标题 s-1',
        workspace: '/work/a',
        updatedAt: 20,
      },
      {
        node: 'studio',
        sessionId: toSessionId('s-2'),
        title: '标题 s-2',
        workspace: '/work/a',
        updatedAt: 30,
      },
    ]);
  });

  it('缺失分片只列可用节点', () => {
    const value = missingShards([nodeA, nodeB], { studio: shard() });
    expect(value).toEqual(['macbook-air-01']);

    const unreachable: NodeRecord = { ...nodeA, status: 'unreachable' };
    expect(missingShards([unreachable], {})).toEqual([]);
  });
});
