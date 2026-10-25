import { describe, expect, it } from 'vitest';

import type { RunRecord } from '../api/generated/RunRecord';
import type { SessionMetadata } from '../api/generated/SessionMetadata';
import {
  lastRunStatus,
  runningEntries,
  sessionMark,
  sessionUnread,
} from './activity';
import { toSessionId } from './ids';
import { emptyNodeState, loadNodeBootstrap, storeSession } from './node-state';
import { reduce } from './reducer';
import type { ClientState } from './reducer';
import type { NodeRecord } from './types';

const nodeA: NodeRecord = { name: 'macbook-air-01', url: 'http://a', enabled: true, status: 'ok', detail: null };
const nodeB: NodeRecord = { name: 'studio', url: 'http://b', enabled: true, status: 'ok', detail: null };

function session(id: string, updatedAt: number, busy = false, queueLength = 0): SessionMetadata {
  return { id, workspace: '/work/a', title: `标题 ${id}`, busy, queueLength, updatedAt };
}

function run(
  id: number,
  sessionId: string,
  status: RunRecord['status'],
  updates: Partial<RunRecord> = {},
): RunRecord {
  return {
    id,
    sessionId,
    status,
    failure: null,
    createdAt: id,
    updatedAt: id,
    ...updates,
  };
}

describe('正在运行面板', () => {
  it('跨节点列出运行中的 session 与队列长度', () => {
    const entries = runningEntries([nodeA, nodeB], {
      'macbook-air-01': {
        ...emptyNodeState,
        sessions: [session('s-1', 10, true), session('s-2', 9)],
        trees: {
          's-1': { activeLeaf: null, nodes: [], queue: [], runs: [run(3, 's-1', 'running')] },
        },
      },
      studio: {
        ...emptyNodeState,
        sessions: [session('s-3', 12, true, 2)],
      },
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

  it('run 结束或失败后不再出现在面板里', () => {
    const shard = {
      ...emptyNodeState,
      sessions: [session('s-1', 10)],
      trees: {
        's-1': {
          activeLeaf: null,
          nodes: [],
          queue: [],
          runs: [run(1, 's-1', 'running', { updatedAt: 5 }), run(1, 's-1', 'succeeded', { updatedAt: 6 })],
        },
      },
    };
    expect(runningEntries([nodeA], { 'macbook-air-01': shard })).toEqual([]);
  });

  it('失败的 run 不留在面板里', () => {
    const shard = {
      ...emptyNodeState,
      sessions: [session('s-2', 10)],
      trees: {
        's-2': { activeLeaf: null, nodes: [], queue: [], runs: [run(2, 's-2', 'failed')] },
      },
    };
    expect(runningEntries([nodeA], { 'macbook-air-01': shard })).toEqual([]);
  });

  it('取最近一次 run 的状态与结局', () => {
    const shard = {
      ...emptyNodeState,
      sessions: [session('s-1', 10)],
      lastSeenAt: { 's-1': 5 },
      trees: {
        's-1': {
          activeLeaf: null,
          nodes: [],
          queue: [],
          runs: [
            run(1, 's-1', 'failed', { updatedAt: 4 }),
            run(2, 's-1', 'succeeded', { updatedAt: 8 }),
            run(3, 's-1', 'cancelled', { updatedAt: 6 }),
          ],
        },
      },
    };
    expect(lastRunStatus(shard, 's-1')).toBe('succeeded');
    expect(sessionMark(shard, 's-1')).toEqual({ unread: true, outcome: 'succeeded' });
    expect(lastRunStatus(shard, 's-2')).toBeNull();
  });
});

describe('未读标记', () => {
  it('首次见到 session 时记为已读，之后更新时间前进才未读', () => {
    const shard = loadNodeBootstrap(emptyNodeState, {
      model: 'm',
      maxEventId: 1,
      workspaces: [],
      sessions: [session('s-1', 10)],
    });
    expect(sessionUnread(shard, 's-1')).toBe(false);
    const updated = storeSession(shard, session('s-1', 20));
    expect(sessionUnread(updated, 's-1')).toBe(true);
  });

  it('按分片记录已读，不影响其它分片', () => {
    const shardA = {
      ...emptyNodeState,
      sessions: [session('s-1', 30)],
      lastSeenAt: { 's-1': 10 },
    };
    const shardB = {
      ...emptyNodeState,
      sessions: [session('s-2', 30)],
      lastSeenAt: { 's-2': 10 },
    };
    const state: ClientState = {
      hub: { nodes: [nodeA, nodeB], selected: 'macbook-air-01', cursor: 1 },
      nodes: { 'macbook-air-01': shardA, studio: shardB },
      connection: 'connected',
      error: null,
    };
    const next = reduce(state, {
      type: 'seen',
      node: 'macbook-air-01',
      sessionId: 's-1',
      at: 30,
    });
    expect(next.nodes['macbook-air-01']?.lastSeenAt['s-1']).toBe(30);
    expect(next.nodes.studio).toBe(shardB);
    expect(sessionUnread(next.nodes['macbook-air-01'] ?? emptyNodeState, 's-1')).toBe(false);
    expect(sessionUnread(next.nodes.studio ?? emptyNodeState, 's-2')).toBe(true);
  });

  it('已读时间只前进，不回退', () => {
    const shard = { ...emptyNodeState, lastSeenAt: { 's-1': 30 } };
    const state: ClientState = {
      hub: { nodes: [nodeA], selected: 'macbook-air-01', cursor: 1 },
      nodes: { 'macbook-air-01': shard },
      connection: 'connected',
      error: null,
    };
    const next = reduce(state, { type: 'seen', node: 'macbook-air-01', sessionId: 's-1', at: 10 });
    expect(next.nodes['macbook-air-01']?.lastSeenAt['s-1']).toBe(30);
  });
});
