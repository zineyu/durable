import { describe, expect, it } from 'vitest';

import type { BootstrapResponse } from '../api/generated/BootstrapResponse';
import type { ContentPart } from '../api/generated/ContentPart';
import type { FactEvent } from '../api/generated/FactEvent';
import type { HostMetadata } from '../api/generated/HostMetadata';
import type { HostSnapshot } from '../api/generated/HostSnapshot';
import type { MessageSkeleton } from '../api/generated/MessageSkeleton';
import type { PartSkeleton } from '../api/generated/PartSkeleton';
import type { RunRecord } from '../api/generated/RunRecord';
import type { SessionMetadata } from '../api/generated/SessionMetadata';
import type { SessionTreeSnapshot } from '../api/generated/SessionTreeSnapshot';
import type { WorkspaceMetadata } from '../api/generated/WorkspaceMetadata';
import { toSessionId } from './ids';
import { missingBodyIds, sessionView } from './derived';
import { loadSessionTree, snapshotCursor } from './state';
import type { ClientState } from './state';
import type { MessagePlaceholder } from './types';
import { initialClientState, reduce } from './reducer';
import type { StreamFactEvent } from './reducer';

const workspaceA: WorkspaceMetadata = { host: 'local', path: '/work/a', registeredAt: 10 };
const workspaceB: WorkspaceMetadata = { host: 'studio', path: '/work/b', registeredAt: 20 };

function host(name: string, status: HostSnapshot['status'] = 'ok'): HostSnapshot {
  return {
    name,
    kind: name === 'local' ? 'local' : 'ssh',
    target: name === 'local' ? null : name,
    enabled: true,
    status,
    detail: status === 'ok' ? null : '隧道已断开',
    latencyMs: null,
  };
}

function hostOf(name: string, enabled = true): HostMetadata {
  return {
    name,
    kind: name === 'local' ? 'local' : 'ssh',
    target: name === 'local' ? null : name,
    enabled,
    createdAt: 20,
    updatedAt: 20,
  };
}

function session(
  id: string,
  workspace: string,
  updatedAt: number,
  extra: Partial<SessionMetadata> = {},
): SessionMetadata {
  return {
    id,
    host: workspace === '/work/b' ? 'studio' : 'local',
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
  hosts: HostSnapshot[] = [host('local'), host('studio')],
): BootstrapResponse {
  return { model: 'gpt-5-codex', maxEventId, hosts, workspaces, sessions };
}

function loaded(): ClientState {
  return reduce(initialClientState, {
    type: 'bootstrap',
    bootstrap: bootstrap(
      [workspaceA, workspaceB],
      [session('s-1', '/work/a', 100), session('s-2', '/work/b', 300)],
    ),
  });
}

function fact(
  id: number,
  event: FactEvent,
  session: SessionMetadata | null = null,
): StreamFactEvent {
  if (session !== null && 'sessionId' in event && event.sessionId !== session.id) {
    throw new Error('夹具的 sessionId 必须与信封的 session 一致');
  }
  return {
    type: 'fact',
    id,
    sessionId: session === null ? null : session.id,
    session,
    timestamp: id,
    event,
  };
}

function streamed(state: ClientState, event: StreamFactEvent): ClientState {
  return reduce(state, { type: 'stream', event });
}

function delta(
  sessionId: string,
  messageId: number,
  value: string,
  kind: 'text' | 'reasoning' = 'text',
): StreamFactEvent {
  return {
    type: 'delta',
    sessionId: toSessionId(sessionId),
    messageId,
    kind,
    text: value,
    timestamp: 1,
  };
}

function bodyOf(state: ClientState, messageId: number): ContentPart[] | null {
  return state.bodies[messageId] ?? null;
}

function placeholdersOf(state: ClientState, sessionId: string): MessagePlaceholder[] {
  return state.placeholders[sessionId] ?? [];
}

/// 派生视图的 transcript 顺序，用于断言两条事件路径得到同一结果。
function transcriptIds(state: ClientState): number[] {
  const view = sessionView(state, 's-1');
  return view === null ? [] : view.transcript.map((entry) => entry.message.id);
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

function withTree(state: ClientState, value: SessionTreeSnapshot): ClientState {
  return reduce(state, { type: 'tree', tree: value });
}

describe('bootstrap', () => {
  it('装配唯一一份状态并记录事件游标', () => {
    const state = loaded();

    expect(state.hosts.map((record) => record.name)).toEqual(['local', 'studio']);
    expect(state.modelName).toBe('gpt-5-codex');
    expect(state.lastEventId).toBe(7);
    expect(state.workspaces.map((workspace) => workspace.path)).toEqual(['/work/a', '/work/b']);
    expect(state.sessions.map((entry) => entry.id)).toEqual(['s-1', 's-2']);
  });

  it('host 状态来自 bootstrap，选中位置不进入状态', () => {
    const state = reduce(initialClientState, {
      type: 'bootstrap',
      bootstrap: bootstrap([], [], 3, [
        host('local', 'unreachable'),
        host('legacy-lab', 'incompatible'),
        host('studio'),
      ]),
    });

    expect(state.hosts.map((record) => record.status)).toEqual([
      'unreachable',
      'incompatible',
      'ok',
    ]);
    expect(state.lastEventId).toBe(3);
    expect('selection' in state).toBe(false);
  });
});

describe('侧栏状态', () => {
  it('注册与移除 workspace 同步增删其 session', () => {
    const state = loaded();

    const registered = streamed(
      state,
      fact(8, {
        kind: 'workspace_registered',
        workspace: { host: 'studio', path: '/work/c', registeredAt: 30 },
      }),
    );
    expect(registered.workspaces.map((workspace) => workspace.path)).toEqual([
      '/work/a',
      '/work/b',
      '/work/c',
    ]);

    const withSession = streamed(
      registered,
      fact(
        9,
        { kind: 'session_created', sessionId: 's-3' },
        session('s-3', '/work/c', 200, { host: 'studio' }),
      ),
    );
    const removed = streamed(
      withSession,
      fact(10, { kind: 'workspace_removed', host: 'studio', path: '/work/c' }),
    );
    expect(removed.workspaces.map((workspace) => workspace.path)).toEqual([
      '/work/a',
      '/work/b',
    ]);
    expect(removed.sessions.map((entry) => entry.id)).toEqual(['s-1', 's-2']);
  });

  it('重命名与删除 session', () => {
    const state = withTree(loaded(), tree('s-1', 7, 1, [message(1, null, 'user')]));

    const renamed = streamed(
      state,
      fact(
        8,
        { kind: 'session_renamed', sessionId: 's-1' },
        session('s-1', '/work/a', 300, { title: '新标题' }),
      ),
    );
    expect(renamed.sessions[0]).toEqual(session('s-1', '/work/a', 300, { title: '新标题' }));

    const deleted = streamed(
      renamed,
      fact(9, { kind: 'session_deleted', sessionId: 's-1' }),
    );
    expect(deleted.sessions.map((entry) => entry.id)).toEqual(['s-2']);
    expect(deleted.trees['s-1']).toBeUndefined();
    expect(deleted.trees['s-2']).toBeUndefined();
  });

  it('移除队列项后减少队列长度与队列视图', () => {
    const state = withTree(loaded(), {
      ...tree('s-1', 7, null, []),
      session: session('s-1', '/work/a', 100, { queueLength: 1 }),
      queue: [{ id: 5, text: '排队', anchor: null, createdAt: 5, pendingSteer: false }],
    });

    const next = streamed(
      state,
      fact(
        8,
        { kind: 'queue_item_removed', sessionId: 's-1', itemId: 5 },
        session('s-1', '/work/a', 108, { queueLength: 0 }),
      ),
    );
    expect(next.sessions[0]?.queueLength).toBe(0);
    expect(next.trees['s-1']?.queue).toEqual([]);
  });

  it('迁移前的队列项移除没有 session 视图时只删队列项', () => {
    const state = withTree(loaded(), {
      ...tree('s-1', 7, null, []),
      session: session('s-1', '/work/a', 100, { queueLength: 1 }),
      queue: [{ id: 5, text: '排队', anchor: null, createdAt: 5, pendingSteer: false }],
    });

    const next = streamed(
      state,
      fact(8, { kind: 'queue_item_removed', sessionId: 's-1', itemId: 5 }),
    );
    expect(next.trees['s-1']?.queue).toEqual([]);
    expect(next.sessions[0]?.queueLength).toBe(1);
  });

  it('插话入队后标记待插入，降级后清除标记', () => {
    const steer = { id: 5, text: '插话', anchor: 3, createdAt: 5, pendingSteer: true };
    const state = withTree(loaded(), tree('s-1', 7, null, []));

    const queued = streamed(
      state,
      fact(
        8,
        { kind: 'message_queued', sessionId: 's-1', item: steer },
        session('s-1', '/work/a', 300, { busy: true, queueLength: 1 }),
      ),
    );
    expect(queued.trees['s-1']?.queue).toEqual([steer]);
    expect(queued.sessions[0]?.queueLength).toBe(1);

    const applied = streamed(
      queued,
      fact(
        9,
        { kind: 'steer_applied', sessionId: 's-1', runId: 1, itemId: 5, messageId: 3 },
        session('s-1', '/work/a', 310, { busy: true, queueLength: 0 }),
      ),
    );
    expect(applied.sessions[0]?.queueLength).toBe(0);

    const downgraded = streamed(
      queued,
      fact(
        9,
        {
          kind: 'steer_downgraded',
          sessionId: 's-1',
          item: { ...steer, pendingSteer: false },
        },
        session('s-1', '/work/a', 310, { queueLength: 1 }),
      ),
    );
    expect(downgraded.trees['s-1']?.queue).toEqual([{ ...steer, pendingSteer: false }]);
  });
});

describe('正文缓存', () => {
  it('缺失的正文按 id 批量请求，同一 id 不重复请求', () => {
    const state = withTree(loaded(), tree('s-1', 7, 2, [
      message(1, null, 'user'),
      message(2, 1, 'assistant'),
    ]));

    expect(missingBodyIds(state, 's-1')).toEqual([1, 2]);

    const requested = reduce(state, { type: 'bodies-request', ids: [1, 2] });
    expect(missingBodyIds(requested, 's-1')).toEqual([]);
  });

  it('拉到的正文按消息 id 缓存，未返回的 id 记为空正文', () => {
    const state = reduce(
      withTree(loaded(), tree('s-1', 7, 1, [message(1, null, 'user')])),
      { type: 'bodies-request', ids: [1, 2] },
    );
    const loadedBodies = reduce(state, {
      type: 'bodies',
      ids: [1, 2],
      messages: [{ id: 1, parts: text('问') }],
    });

    expect(bodyOf(loadedBodies, 1)).toEqual(text('问'));
    expect(bodyOf(loadedBodies, 2)).toEqual([]);
    expect(bodyOf(loadedBodies, 3)).toBeNull();
    expect(loadedBodies.bodyRequests).toEqual([]);
  });

  it('message_updated 使该消息的正文失效，其它消息不变', () => {
    const state = reduce(
      withTree(loaded(), tree('s-1', 7, 3, [
        message(1, null, 'user'),
        message(2, 1, 'assistant', { parts: parts(['tool_call']) }),
        message(3, 2, 'tool', { parts: parts(['tool_result']) }),
      ])),
      { type: 'bodies-request', ids: [2, 3] },
    );
    const withBodies = reduce(state, {
      type: 'bodies',
      ids: [2, 3],
      messages: [
        { id: 2, parts: [{ kind: 'tool_call', callId: 'call_1', name: 'read_file', arguments: '{}', providerMeta: null }] },
        { id: 3, parts: [{ kind: 'tool_result', callId: 'call_1', state: { status: 'running' }, providerMeta: null }] },
      ],
    });

    const updated = streamed(
      withBodies,
      fact(
        8,
        {
          kind: 'message_updated',
          sessionId: 's-1',
          message: message(3, 2, 'tool', { parts: parts(['tool_result']) }),
        },
        session('s-1', '/work/a', 200),
      ),
    );

    expect(bodyOf(updated, 2)).not.toBeNull();
    expect(bodyOf(updated, 3)).toBeNull();
    expect(missingBodyIds(updated, 's-1')).toEqual([1, 3]);
  });

  it('拉取失败的消息不再自动重试，快照重载后恢复', () => {
    const state = withTree(loaded(), tree('s-1', 7, 1, [message(1, null, 'user')]));
    const requested = reduce(state, { type: 'bodies-request', ids: [1] });
    const failed = reduce(requested, { type: 'bodies-failed', ids: [1] });

    expect(missingBodyIds(failed, 's-1')).toEqual([]);

    const reloaded = withTree(failed, tree('s-1', 7, 1, [message(1, null, 'user')]));
    expect(missingBodyIds(reloaded, 's-1')).toEqual([1]);
  });

  it('快照重载清空在途请求，被中断的正文可以重新拉取', () => {
    const state = withTree(loaded(), tree('s-1', 7, 1, [message(1, null, 'user')]));
    const requested = reduce(state, { type: 'bodies-request', ids: [1] });
    expect(missingBodyIds(requested, 's-1')).toEqual([]);

    const reloaded = withTree(requested, tree('s-1', 7, 1, [message(1, null, 'user')]));
    expect(missingBodyIds(reloaded, 's-1')).toEqual([1]);
  });

  it('失效后迟到的正文响应被丢弃，并重新拉取', () => {
    const state = withTree(loaded(), tree('s-1', 7, 2, [
      message(1, null, 'user'),
      message(2, 1, 'tool', { parts: parts(['tool_result']) }),
    ]));
    const requested = reduce(state, { type: 'bodies-request', ids: [2] });
    const invalidated = streamed(
      requested,
      fact(
        8,
        {
          kind: 'message_updated',
          sessionId: 's-1',
          message: message(2, 1, 'tool', { parts: parts(['tool_result']) }),
        },
        session('s-1', '/work/a', 200),
      ),
    );
    const late = reduce(invalidated, {
      type: 'bodies',
      ids: [2],
      messages: [
        { id: 2, parts: [{ kind: 'tool_result', callId: 'call_1', state: { status: 'running' }, providerMeta: null }] },
      ],
    });

    expect(bodyOf(late, 2)).toBeNull();
    expect(missingBodyIds(late, 's-1')).toEqual([1, 2]);
  });

  it('拉取失败的消息可以显式重试', () => {
    const state = withTree(loaded(), tree('s-1', 7, 1, [message(1, null, 'user')]));
    const failed = reduce(
      reduce(state, { type: 'bodies-request', ids: [1] }),
      { type: 'bodies-failed', ids: [1] },
    );
    expect(missingBodyIds(failed, 's-1')).toEqual([]);

    const retried = reduce(failed, { type: 'bodies-retry', ids: [1] });
    expect(missingBodyIds(retried, 's-1')).toEqual([1]);
  });

  it('删除 session 时丢弃它的正文缓存', () => {
    const state = reduce(
      reduce(withTree(loaded(), tree('s-1', 7, 1, [message(1, null, 'user')])), {
        type: 'bodies-request',
        ids: [1],
      }),
      {
        type: 'bodies',
        ids: [1],
        messages: [{ id: 1, parts: text('问') }],
      },
    );
    const deleted = streamed(state, fact(8, { kind: 'session_deleted', sessionId: 's-1' }));

    expect(bodyOf(deleted, 1)).toBeNull();
    expect(deleted.bodyRequests).toEqual([]);
    expect(deleted.bodyFailures).toEqual([]);
  });
});

describe('流式增量与占位节点', () => {
  const asked = (): ClientState =>
    withTree(loaded(), tree('s-1', 7, 1, [message(1, null, 'user')]));

  it('未定型的增量建本地占位，并合并同类块', () => {
    let state = streamed(asked(), delta('s-1', 9, '你'));
    state = streamed(state, delta('s-1', 9, '好'));
    state = streamed(state, delta('s-1', 9, '想', 'reasoning'));

    expect(placeholdersOf(state, 's-1')).toEqual([
      {
        id: 9,
        parts: [
          { kind: 'text', text: '你好', providerMeta: null },
          { kind: 'reasoning', text: '想', providerMeta: null },
        ],
      },
    ]);
    expect(state.trees['s-1']?.nodes.map((node) => node.id)).toEqual([1]);
  });

  it('message_appended 用同一 id 的骨架接管占位', () => {
    const streaming = streamed(asked(), delta('s-1', 9, '部分'));
    const appended = streamed(
      streamed(
        streaming,
        fact(
          9,
          {
            kind: 'message_appended',
            sessionId: 's-1',
            message: message(9, 1, 'assistant', { runId: 1, parts: parts(['text']) }),
          },
          session('s-1', '/work/a', 200, { busy: true }),
        ),
      ),
      fact(
        10,
        { kind: 'active_leaf_changed', sessionId: 's-1', activeLeaf: 9 },
        session('s-1', '/work/a', 210, { busy: true }),
      ),
    );

    expect(placeholdersOf(appended, 's-1')).toEqual([]);
    expect(missingBodyIds(appended, 's-1')).toEqual([1, 9]);
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
      fact(
        8,
        { kind: 'run_started', sessionId: 's-1', run: running },
        session('s-1', '/work/a', 200, { busy: true }),
      ),
    );
    state = streamed(state, delta('s-1', 9, '被打断的回答。'));
    expect(placeholdersOf(state, 's-1')).toHaveLength(1);

    state = streamed(
      state,
      fact(
        9,
        {
          kind: 'run_finished',
          sessionId: 's-1',
          run: { ...running, status: 'cancelled', updatedAt: 300 },
        },
        session('s-1', '/work/a', 300),
      ),
    );
    expect(placeholdersOf(state, 's-1')).toEqual([]);
  });

  it('快照重载丢弃占位', () => {
    const streaming = streamed(asked(), delta('s-1', 9, '部分'));
    const reloaded = withTree(streaming, tree('s-1', 7, 1, [message(1, null, 'user')]));

    expect(placeholdersOf(reloaded, 's-1')).toEqual([]);
  });

  it('已入树的同一 id 不再累积增量', () => {
    const state = streamed(
      withTree(loaded(), tree('s-1', 7, 2, [
        message(1, null, 'user'),
        message(2, 1, 'assistant', { runId: 1 }),
      ])),
      delta('s-1', 2, '迟到增量'),
    );
    expect(placeholdersOf(state, 's-1')).toEqual([]);
  });

  it('delta 不推进事件游标', () => {
    const state = streamed(asked(), delta('s-1', 9, '你'));
    expect(state.lastEventId).toBe(7);
  });
});

describe('单一游标与多 session 状态', () => {
  it('快照游标取较老水位，无基线时取快照水位', () => {
    expect(snapshotCursor(null, 7)).toBe(7);
    expect(snapshotCursor(9, 7)).toBe(7);
    expect(snapshotCursor(5, 9)).toBe(5);
  });

  it('重复到达的事实只应用一次', () => {
    const state = withTree(loaded(), tree('s-1', 7, 1, [message(1, null, 'user')]));
    const event = fact(
      8,
      { kind: 'session_renamed', sessionId: 's-1' },
      session('s-1', '/work/a', 200, { title: '改名' }),
    );

    const once = streamed(state, event);
    const twice = streamed(once, event);
    expect(twice).toBe(once);
    expect(twice.sessions[0]?.title).toBe('改名');
  });

  it('乱序到达的旧事实不改变已有状态', () => {
    const newer = streamed(
      loaded(),
      fact(
        110,
        { kind: 'session_renamed', sessionId: 's-1' },
        session('s-1', '/work/a', 300, { title: '新' }),
      ),
    );
    const older = streamed(
      newer,
      fact(
        105,
        { kind: 'session_renamed', sessionId: 's-1' },
        session('s-1', '/work/a', 200, { title: '旧' }),
      ),
    );

    expect(older.sessions[0]?.title).toBe('新');
  });

  it('不同 session 的事件交错到达时互不覆盖', () => {
    const state = streamed(
      streamed(
        loaded(),
        fact(108, { kind: 'session_created', sessionId: 's-3' }, session('s-3', '/work/a', 200)),
      ),
      fact(208, { kind: 'session_created', sessionId: 's-4' }, session('s-4', '/work/b', 400)),
    );

    expect(state.sessions.map((entry) => entry.id)).toEqual(['s-1', 's-2', 's-3', 's-4']);
  });

  it('占位按 session 隔离，同名消息 id 不串写', () => {
    let state = withTree(
      withTree(loaded(), tree('s-1', 7, 2, [
        message(1, null, 'user'),
        message(2, 1, 'assistant', { runId: 1 }),
      ])),
      {
        ...tree('s-2', 7, 5, [message(5, null, 'assistant', { runId: 1 })]),
        session: session('s-2', '/work/b', 300),
      },
    );

    state = streamed(state, delta('s-2', 6, 'B 输出'));
    state = streamed(state, delta('s-1', 2, 'A 输出'));

    expect(placeholdersOf(state, 's-2')).toEqual([
      { id: 6, parts: [{ kind: 'text', text: 'B 输出', providerMeta: null }] },
    ]);
    expect(placeholdersOf(state, 's-1')).toEqual([]);
  });

  it('未知 session 的增量与重试提示不改变状态', () => {
    const state = loaded();
    expect(streamed(state, delta('missing', 9, '文本'))).toBe(state);
    expect(
      streamed(state, {
        type: 'retry',
        sessionId: toSessionId('missing'),
        attempt: 1,
        maxAttempts: 3,
        delayMs: 2000,
      }),
    ).toBe(state);
  });
});

describe('host 表变更', () => {
  it('host_registered 增加不可达记录，host_status_changed 标记可用', () => {
    const registered = streamed(
      loaded(),
      fact(8, { kind: 'host_registered', host: hostOf('legacy-lab') }),
    );
    expect(registered.hosts.at(-1)).toEqual({
      name: 'legacy-lab',
      kind: 'ssh',
      target: 'legacy-lab',
      enabled: true,
      status: 'unreachable',
      detail: null,
      latencyMs: null,
    });

    const ok = streamed(
      registered,
      fact(9, { kind: 'host_status_changed', host: 'legacy-lab', status: 'ok', detail: null }),
    );
    expect(ok.hosts.at(-1)?.status).toBe('ok');
    expect(ok.lastEventId).toBe(9);
  });

  it('host 不可用时保留 session，由派生视图隐藏列表', () => {
    const state = streamed(
      loaded(),
      fact(8, { kind: 'host_status_changed', host: 'studio', status: 'unreachable', detail: '隧道已断开' }),
    );

    expect(state.hosts.find((record) => record.name === 'studio')).toMatchObject({
      status: 'unreachable',
      detail: '隧道已断开',
    });
    expect(state.sessions.map((entry) => entry.id)).toEqual(['s-1', 's-2']);
  });

  it('host_removed 删除记录', () => {
    const removed = streamed(loaded(), fact(8, { kind: 'host_removed', host: 'studio' }));

    expect(removed.hosts.map((record) => record.name)).toEqual(['local']);
    expect(removed.lastEventId).toBe(8);
  });

  it('host_updated 只改元数据，不动状态与 session', () => {
    const state = loaded();
    const updated = streamed(
      state,
      fact(8, { kind: 'host_updated', host: hostOf('studio', false) }),
    );

    expect(updated.hosts.find((record) => record.name === 'studio')).toMatchObject({
      kind: 'ssh',
      target: 'studio',
      enabled: false,
      status: 'ok',
    });
    expect(updated.sessions.map((entry) => entry.id)).toEqual(['s-1', 's-2']);
    expect(updated.lastEventId).toBe(8);
  });
});

describe('快照水位与回放恢复', () => {
  const question = message(1, null, 'user');
  const answer = message(2, 1, 'assistant', { runId: 1 });

  it('tree 快照落后于已应用的事实时补齐尾部', () => {
    const appended = streamed(
      streamed(
        loaded(),
        fact(
          108,
          { kind: 'message_appended', sessionId: 's-1', message: question },
          session('s-1', '/work/a', 108),
        ),
      ),
      fact(
        109,
        { kind: 'message_appended', sessionId: 's-1', message: answer },
        session('s-1', '/work/a', 109),
      ),
    );
    expect(appended.lastEventId).toBe(109);

    const landed = withTree(appended, tree('s-1', 108, null, [question]));

    expect(landed.lastEventId).toBe(109);
    expect(landed.trees['s-1']?.nodes.map((entry) => entry.id)).toEqual([1, 2]);
    expect(landed.trees['s-1']?.activeLeaf).toBeNull();
  });

  it('tree 快照水位高于已应用水位时不推进水位', () => {
    const state = streamed(
      loaded(),
      fact(
        108,
        { kind: 'session_renamed', sessionId: 's-1' },
        session('s-1', '/work/a', 108, { title: '新' }),
      ),
    );
    const landed = withTree(state, tree('s-1', 400, null, []));

    expect(landed.lastEventId).toBe(108);
    expect(landed.sessions.map((entry) => entry.id)).toEqual(['s-1', 's-2']);
  });

  it('一个 session 的树快照不改变另一 session 的树', () => {
    const state = streamed(
      loaded(),
      fact(
        308,
        { kind: 'session_renamed', sessionId: 's-2' },
        session('s-2', '/work/b', 308, { title: 'B 改名' }),
      ),
    );
    const landed = withTree(state, tree('s-1', 7, null, []));

    expect(landed.sessions.find((entry) => entry.id === 's-2')?.title).toBe('B 改名');
    expect(landed.lastEventId).toBe(308);
    expect(landed.trees['s-2']).toBeUndefined();
  });

  it('回放事件构建的 transcript 等于 tree 快照', () => {
    const base = loaded();
    let replayed = streamed(
      base,
      fact(
        108,
        { kind: 'message_appended', sessionId: 's-1', message: question },
        session('s-1', '/work/a', 108),
      ),
    );
    replayed = streamed(
      replayed,
      fact(
        109,
        { kind: 'message_appended', sessionId: 's-1', message: answer },
        session('s-1', '/work/a', 109),
      ),
    );
    replayed = streamed(
      replayed,
      fact(
        110,
        { kind: 'active_leaf_changed', sessionId: 's-1', activeLeaf: 2 },
        session('s-1', '/work/a', 110),
      ),
    );

    const snapshot = withTree(base, tree('s-1', 110, 2, [question, answer]));

    expect(transcriptIds(replayed)).toEqual(transcriptIds(snapshot));
  });

  it('加载快照后重放新事实等于从零构建', () => {
    const base = loaded();
    const append = fact(
      108,
      { kind: 'message_appended', sessionId: 's-1', message: question },
      session('s-1', '/work/a', 108),
    );
    const leaf = fact(
      109,
      { kind: 'active_leaf_changed', sessionId: 's-1', activeLeaf: 1 },
      session('s-1', '/work/a', 109),
    );

    const live = streamed(streamed(base, append), leaf);
    const landed = withTree(live, tree('s-1', 107, null, []));
    const fromScratch = streamed(
      streamed(withTree(base, tree('s-1', 107, null, [])), append),
      leaf,
    );

    expect(landed.trees).toEqual(fromScratch.trees);
  });
});

describe('连接状态与错误', () => {
  it('记录连接状态与错误横幅', () => {
    const connected = reduce(initialClientState, { type: 'connection', connection: 'connected' });
    expect(connected.connection).toBe('connected');

    const failed = reduce(connected, {
      type: 'error',
      error: { code: 'host_unavailable', message: 'host 不可用' },
    });
    expect(failed.error).toEqual({ code: 'host_unavailable', message: 'host 不可用' });

    expect(reduce(failed, { type: 'dismiss-error' }).error).toBeNull();
  });
});

describe('状态辅助', () => {
  it('loadSessionTree 直接作用于唯一状态', () => {
    const state = loadSessionTree(loaded(), tree('s-2', 9, null, []));
    expect(state.trees['s-2']?.activeLeaf).toBeNull();
    expect(state.lastEventId).toBe(7);
  });

  it('状态整体保留快照的协议字段，只取走信封字段', () => {
    type ExtendedTree = SessionTreeSnapshot & { futureField: string };
    const extended: ExtendedTree = { ...tree('s-2', 9, null, []), futureField: 'x' };

    const state = loadSessionTree(loaded(), extended);

    expect(state.trees['s-2']).toEqual({
      activeLeaf: null,
      nodes: [],
      queue: [],
      runs: [],
      futureField: 'x',
    });
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
    sessionId: string,
    attempt = 1,
    maxAttempts = 3,
    delayMs = 2000,
  ): StreamFactEvent {
    return {
      type: 'retry',
      sessionId: toSessionId(sessionId),
      attempt,
      maxAttempts,
      delayMs,
    };
  }

  function withRunning(): ClientState {
    const state = withTree(loaded(), tree('s-1', 7, 2, [
      message(1, null, 'user'),
      message(2, 1, 'assistant', { runId: 1 }),
    ]));
    return streamed(
      state,
      fact(
        108,
        { kind: 'run_started', sessionId: 's-1', run: runningRun },
        session('s-1', '/work/a', 200, { busy: true }),
      ),
    );
  }

  it('按 session 记录重试等待，不推进事件游标', () => {
    const state = withRunning();
    const cursor = state.lastEventId;

    const retrying = streamed(state, retryEvent('s-1'));

    expect(retrying.retries['s-1']).toEqual({
      attempt: 1,
      maxAttempts: 3,
      delayMs: 2000,
    });
    expect(retrying.lastEventId).toBe(cursor);
  });

  it('文本重新流动后清除等待', () => {
    const retrying = streamed(withRunning(), retryEvent('s-1'));
    const streaming = streamed(retrying, delta('s-1', 2, '恢复'));

    expect(streaming.retries['s-1']).toBeUndefined();
  });

  it('run 结束与 session 树重载都会清除重试等待', () => {
    const retrying = streamed(withRunning(), retryEvent('s-1'));
    const finished = streamed(
      retrying,
      fact(
        109,
        {
          kind: 'run_finished',
          sessionId: 's-1',
          run: {
            ...runningRun,
            status: 'failed',
            failure: { code: 'model_rate_limited', message: '模型返回非成功状态 429: 限流' },
            updatedAt: 300,
          },
        },
        session('s-1', '/work/a', 300),
      ),
    );

    expect(finished.retries['s-1']).toBeUndefined();

    const reloaded = withTree(
      retrying,
      tree('s-1', 9, 2, [
        message(1, null, 'user'),
        message(2, 1, 'assistant', { runId: 1 }),
      ]),
    );
    expect(reloaded.retries['s-1']).toBeUndefined();
  });
});
