import { describe, expect, it } from 'vitest';

import type { NodeName } from '../api/generated/NodeName';
import { toSessionId } from './ids';
import { buildPalette, filterPalette, stepPaletteIndex } from './command-palette';
import type { PaletteInput } from './command-palette';
import type { NodeRecord, PaletteSession, SessionSummary } from './types';

const nodeA: NodeRecord = { name: 'macbook-air-01', url: 'http://a', enabled: true, status: 'ok', detail: null };
const nodeB: NodeRecord = { name: 'studio', url: 'http://b', enabled: true, status: 'ok', detail: null };
const nodeC: NodeRecord = {
  name: 'retired',
  url: 'http://c',
  enabled: true,
  status: 'unreachable',
  detail: '隧道断开',
};

function paletteSession(
  node: NodeName,
  id: string,
  title: string,
  updatedAt: number,
): PaletteSession {
  return { node, sessionId: toSessionId(id), title, workspace: '/work/a', updatedAt };
}

function summary(
  id: string,
  title: string,
  updatedAt: number,
  state: SessionSummary['state'] = 'idle',
): SessionSummary {
  return {
    id: toSessionId(id),
    title,
    state,
    queuedCount: 0,
    updatedAt,
    unread: false,
    outcome: null,
  };
}

function input(overrides: Partial<PaletteInput> = {}): PaletteInput {
  return {
    nodes: [nodeA, nodeB, nodeC],
    sessions: [
      paletteSession('macbook-air-01', 's-1', '迁移与恢复', 20),
      paletteSession('macbook-air-01', 's-2', '反代与内嵌', 10),
      paletteSession('studio', 's-3', '类型生成一致性', 30),
    ],
    selectedNode: 'macbook-air-01',
    selectedSession: summary('s-2', '反代与内嵌', 10),
    ...overrides,
  };
}

describe('命令面板条目', () => {
  it('列出全部节点的 session，最近更新的在前，并写明节点', () => {
    const entries = buildPalette(input());
    const sessions = entries.filter((entry) => entry.group === 'session');
    expect(sessions.map((entry) => entry.label)).toEqual([
      '类型生成一致性',
      '迁移与恢复',
      '反代与内嵌',
    ]);
    expect(sessions[0]?.detail).toContain('studio');
    expect(sessions[0]?.action).toEqual({
      kind: 'session',
      node: 'studio',
      sessionId: toSessionId('s-3'),
    });
  });

  it('动作包含注册节点、注册 workspace 与停止当前 run', () => {
    const entries = buildPalette(
      input({
        selectedNode: 'studio',
        selectedSession: summary('s-3', '类型生成一致性', 30, 'running'),
      }),
    );
    const labels = entries.filter((entry) => entry.group === 'action').map((entry) => entry.label);
    expect(labels).toEqual(['注册节点', '注册 workspace', '停止 run']);
  });

  it('节点不可用或 session 空闲时不提供对应动作', () => {
    const labels = buildPalette(input({ selectedNode: 'retired', selectedSession: null }))
      .filter((entry) => entry.group === 'action')
      .map((entry) => entry.label);
    expect(labels).toEqual(['注册节点']);
  });

  it('节点分组列出节点与状态', () => {
    const entries = buildPalette(input());
    const nodes = entries.filter((entry) => entry.group === 'node');
    expect(nodes.map((entry) => entry.action)).toEqual([
      { kind: 'node', node: 'macbook-air-01' },
      { kind: 'node', node: 'studio' },
      { kind: 'node', node: 'retired' },
    ]);
    expect(nodes[2]?.detail).toContain('不可达');
  });
});

describe('命令面板过滤', () => {
  it('空查询保留全部条目', () => {
    const entries = buildPalette(input());
    expect(filterPalette(entries, '  ')).toHaveLength(entries.length);
  });

  it('按标题与节点子序列模糊匹配', () => {
    const entries = buildPalette(input());
    expect(filterPalette(entries, '迁移').map((entry) => entry.label)).toContain('迁移与恢复');
    expect(filterPalette(entries, 'studio').map((entry) => entry.label)).toContain(
      '类型生成一致性',
    );
    expect(
      filterPalette(entries, 'mba').map((entry) => entry.action),
    ).toContainEqual({ kind: 'node', node: 'macbook-air-01' });
  });

  it('没有匹配时返回空列表', () => {
    expect(filterPalette(buildPalette(input()), 'zzzz')).toEqual([]);
  });
});

describe('命令面板选择', () => {
  it('上下移动并在两端环绕', () => {
    expect(stepPaletteIndex(0, 1, 3)).toBe(1);
    expect(stepPaletteIndex(2, 1, 3)).toBe(0);
    expect(stepPaletteIndex(0, -1, 3)).toBe(2);
  });

  it('没有条目时返回 0', () => {
    expect(stepPaletteIndex(0, 1, 0)).toBe(0);
  });
});
