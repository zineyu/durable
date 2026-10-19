import { describe, expect, it } from 'vitest';

import type { BootstrapResponse } from '../api/generated/BootstrapResponse';
import type { FactEvent } from '../api/generated/FactEvent';
import type { SessionMetadata } from '../api/generated/SessionMetadata';
import type { WorkspaceMetadata } from '../api/generated/WorkspaceMetadata';
import { toSessionId } from './ids';
import { applyEvent, createSidebarState, sidebarWorkspaces } from './reducer';

const workspaceA: WorkspaceMetadata = { path: '/work/a', registeredAt: 10 };
const workspaceB: WorkspaceMetadata = { path: '/work/b', registeredAt: 20 };

function session(id: string, workspace: string, updatedAt: number, extra: Partial<SessionMetadata> = {}): SessionMetadata {
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

function bootstrap(
  workspaces: WorkspaceMetadata[],
  sessions: SessionMetadata[],
): BootstrapResponse {
  return { model: 'gpt-5-codex', maxEventId: 7, workspaces, sessions };
}

describe('侧栏状态', () => {
  it('按 workspace 注册顺序与 session 最近更新时间排序', () => {
    const state = createSidebarState(
      bootstrap(
        [workspaceB, workspaceA],
        [
          session('s-old', '/work/a', 100),
          session('s-new', '/work/a', 300),
          session('s-mid', '/work/a', 200),
          session('s-b', '/work/b', 50),
        ],
      ),
    );

    expect(state.modelName).toBe('gpt-5-codex');
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
        sessions: [
          { id: toSessionId('s-b'), title: '标题 s-b', state: 'idle', queuedCount: 0 },
        ],
      },
    ]);
  });

  it('由 busy 与队列长度推导忙碌状态', () => {
    const state = createSidebarState(
      bootstrap(
        [workspaceA],
        [
          session('s-running', '/work/a', 100, { busy: true }),
          session('s-queued', '/work/a', 200, { queueLength: 2 }),
          session('s-running-queued', '/work/a', 300, { busy: true, queueLength: 1 }),
          session('s-idle', '/work/a', 400),
        ],
      ),
    );

    expect(sidebarWorkspaces(state)).toEqual([
      {
        id: '/work/a',
        path: '/work/a',
        sessions: [
          { id: toSessionId('s-idle'), title: '标题 s-idle', state: 'idle', queuedCount: 0 },
          {
            id: toSessionId('s-running-queued'),
            title: '标题 s-running-queued',
            state: 'running',
            queuedCount: 1,
          },
          { id: toSessionId('s-queued'), title: '标题 s-queued', state: 'queued', queuedCount: 2 },
          { id: toSessionId('s-running'), title: '标题 s-running', state: 'running', queuedCount: 0 },
        ],
      },
    ]);
  });

  it('注册 workspace 后按注册时间插入', () => {
    const state = createSidebarState(bootstrap([workspaceB], [session('s-b', '/work/b', 100)]));

    const next = applyEvent(state, {
      kind: 'workspace_registered',
      workspace: workspaceA,
    });

    expect(sidebarWorkspaces(next).map((workspace) => workspace.path)).toEqual([
      '/work/a',
      '/work/b',
    ]);
  });

  it('重复注册同一 workspace 不产生第二个分组', () => {
    const state = createSidebarState(bootstrap([workspaceA], []));

    const next = applyEvent(state, {
      kind: 'workspace_registered',
      workspace: workspaceA,
    });

    expect(sidebarWorkspaces(next)).toHaveLength(1);
  });

  it('移除 workspace 后同时移除其 session', () => {
    const state = createSidebarState(
      bootstrap([workspaceA, workspaceB], [session('s-a', '/work/a', 100), session('s-b', '/work/b', 90)]),
    );

    const next = applyEvent(state, { kind: 'workspace_removed', path: '/work/a' });

    expect(sidebarWorkspaces(next)).toEqual([
      {
        id: '/work/b',
        path: '/work/b',
        sessions: [{ id: toSessionId('s-b'), title: '标题 s-b', state: 'idle', queuedCount: 0 }],
      },
    ]);
  });

  it('创建 session 后置顶并显示忙碌状态', () => {
    const state = createSidebarState(bootstrap([workspaceA], [session('s-old', '/work/a', 100)]));

    const next = applyEvent(state, {
      kind: 'session_created',
      session: session('s-new', '/work/a', 200, { busy: true }),
    });

    expect(sidebarWorkspaces(next)[0]?.sessions).toEqual([
      { id: toSessionId('s-new'), title: '标题 s-new', state: 'running', queuedCount: 0 },
      { id: toSessionId('s-old'), title: '标题 s-old', state: 'idle', queuedCount: 0 },
    ]);
  });

  it('重命名 session 后更新标题并按更新时间置顶', () => {
    const state = createSidebarState(
      bootstrap([workspaceA], [session('s-1', '/work/a', 200), session('s-2', '/work/a', 100)]),
    );

    const next = applyEvent(state, {
      kind: 'session_renamed',
      session: session('s-2', '/work/a', 300, { title: '新标题' }),
    });

    expect(sidebarWorkspaces(next)[0]?.sessions).toEqual([
      { id: toSessionId('s-2'), title: '新标题', state: 'idle', queuedCount: 0 },
      { id: toSessionId('s-1'), title: '标题 s-1', state: 'idle', queuedCount: 0 },
    ]);
  });

  it('删除 session 后不再出现在侧栏', () => {
    const state = createSidebarState(
      bootstrap([workspaceA], [session('s-1', '/work/a', 200), session('s-2', '/work/a', 100)]),
    );

    const next = applyEvent(state, { kind: 'session_deleted', sessionId: 's-1' });

    expect(sidebarWorkspaces(next)[0]?.sessions).toEqual([
      { id: toSessionId('s-2'), title: '标题 s-2', state: 'idle', queuedCount: 0 },
    ]);
  });

  it('归约不修改原状态', () => {
    const state = createSidebarState(bootstrap([workspaceA], [session('s-1', '/work/a', 100)]));

    const event: FactEvent = { kind: 'session_deleted', sessionId: 's-1' };
    const next = applyEvent(state, event);

    expect(sidebarWorkspaces(state)[0]?.sessions).toHaveLength(1);
    expect(sidebarWorkspaces(next)[0]?.sessions).toHaveLength(0);
  });
  it('空 bootstrap 得到空侧栏与模型名', () => {
    const state = createSidebarState(bootstrap([], []));

    expect(state.modelName).toBe('gpt-5-codex');
    expect(sidebarWorkspaces(state)).toEqual([]);
  });

  it('两个 workspace 的 session 事件互不干扰', () => {
    const state = createSidebarState(
      bootstrap(
        [workspaceA, workspaceB],
        [session('s-a1', '/work/a', 100), session('s-b1', '/work/b', 200)],
      ),
    );

    const created = applyEvent(state, {
      kind: 'session_created',
      session: session('s-a2', '/work/a', 300),
    });
    const renamed = applyEvent(created, {
      kind: 'session_renamed',
      session: session('s-b1', '/work/b', 400, { title: 'B 的新标题', busy: true }),
    });

    expect(sidebarWorkspaces(renamed)).toEqual([
      {
        id: '/work/a',
        path: '/work/a',
        sessions: [
          { id: toSessionId('s-a2'), title: '标题 s-a2', state: 'idle', queuedCount: 0 },
          { id: toSessionId('s-a1'), title: '标题 s-a1', state: 'idle', queuedCount: 0 },
        ],
      },
      {
        id: '/work/b',
        path: '/work/b',
        sessions: [
          { id: toSessionId('s-b1'), title: 'B 的新标题', state: 'running', queuedCount: 0 },
        ],
      },
    ]);
  });

  it('重命名忙碌 session 保留活动 run 与队列长度', () => {    const state = createSidebarState(
      bootstrap([workspaceA], [session('s-1', '/work/a', 100, { busy: true, queueLength: 2 })]),
    );

    const next = applyEvent(state, {
      kind: 'session_renamed',
      session: session('s-1', '/work/a', 200, { title: '重命名后', busy: true, queueLength: 2 }),
    });

    expect(sidebarWorkspaces(next)[0]?.sessions).toEqual([
      { id: toSessionId('s-1'), title: '重命名后', state: 'running', queuedCount: 2 },
    ]);
  });

  it('追加消息后按更新时间把 session 置顶', () => {
    const state = createSidebarState(
      bootstrap(
        [workspaceA],
        [session('s-1', '/work/a', 100), session('s-2', '/work/a', 200)],
      ),
    );

    const next = applyEvent(state, {
      kind: 'message_appended',
      session: session('s-1', '/work/a', 300),
      message: { id: 3, parentId: null, role: 'user', text: '新消息', createdAt: 300 },
    });

    expect(sidebarWorkspaces(next)[0]?.sessions).toEqual([
      { id: toSessionId('s-1'), title: '标题 s-1', state: 'idle', queuedCount: 0 },
      { id: toSessionId('s-2'), title: '标题 s-2', state: 'idle', queuedCount: 0 },
    ]);
  });
});
