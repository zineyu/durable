import { describe, expect, it } from 'vitest';

import { toSessionId } from './ids';
import { buildPalette, filterPalette, stepPaletteIndex } from './command-palette';
import type { PaletteInput } from './command-palette';
import type { HostRecord, PaletteSession, SessionSummary } from './types';

const hostA: HostRecord = {
  name: 'macbook-air-01',
  kind: 'local',
  target: null,
  enabled: true,
  status: 'ok',
  detail: null,
  latencyMs: 4,
};
const hostB: HostRecord = {
  name: 'studio',
  kind: 'ssh',
  target: 'studio',
  enabled: true,
  status: 'ok',
  detail: null,
  latencyMs: 32,
};
const hostC: HostRecord = {
  name: 'retired',
  kind: 'ssh',
  target: 'retired',
  enabled: true,
  status: 'unreachable',
  detail: '隧道断开',
  latencyMs: null,
};

function paletteSession(
  host: string,
  id: string,
  title: string,
  updatedAt: number,
): PaletteSession {
  return { host, sessionId: toSessionId(id), title, workspace: '/work/a', updatedAt };
}

function summary(
  host: string,
  id: string,
  title: string,
  updatedAt: number,
  state: SessionSummary['state'] = 'idle',
): SessionSummary {
  return {
    id: toSessionId(id),
    host,
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
    hosts: [hostA, hostB, hostC],
    sessions: [
      paletteSession('macbook-air-01', 's-1', '迁移与恢复', 20),
      paletteSession('macbook-air-01', 's-2', '反代与内嵌', 10),
      paletteSession('studio', 's-3', '类型生成一致性', 30),
    ],
    selectedHost: 'macbook-air-01',
    selectedSession: summary('macbook-air-01', 's-2', '反代与内嵌', 10),
    ...overrides,
  };
}

describe('命令面板条目', () => {
  it('session 按 host 分组，组内最近更新的在前', () => {
    const sessions = buildPalette(input()).filter((entry) => entry.group === 'session');
    expect(sessions.map((entry) => entry.label)).toEqual([
      '迁移与恢复',
      '反代与内嵌',
      '类型生成一致性',
    ]);
    expect(sessions.map((entry) => entry.heading)).toEqual([
      'macbook-air-01',
      'macbook-air-01',
      'studio',
    ]);
    expect(sessions[2]?.detail).toContain('studio');
    expect(sessions[2]?.action).toEqual({
      kind: 'session',
      sessionId: toSessionId('s-3'),
    });
  });

  it('动作包含注册 host、注册 workspace 与停止当前 run', () => {
    const entries = buildPalette(
      input({
        selectedHost: 'studio',
        selectedSession: summary('studio', 's-3', '类型生成一致性', 30, 'running'),
      }),
    );
    const labels = entries.filter((entry) => entry.group === 'action').map((entry) => entry.label);
    expect(labels).toEqual(['注册 host', '注册 workspace', '停止 run']);
  });

  it('host 不可用或 session 空闲时不提供对应动作', () => {
    const labels = buildPalette(input({ selectedHost: 'retired', selectedSession: null }))
      .filter((entry) => entry.group === 'action')
      .map((entry) => entry.label);
    expect(labels).toEqual(['注册 host']);
  });

  it('host 分组列出 host、状态与延迟', () => {
    const hosts = buildPalette(input()).filter((entry) => entry.group === 'host');
    expect(hosts.map((entry) => entry.action)).toEqual([
      { kind: 'host', host: 'macbook-air-01' },
      { kind: 'host', host: 'studio' },
      { kind: 'host', host: 'retired' },
    ]);
    expect(hosts[2]?.detail).toContain('不可达');
    expect(hosts[1]?.detail).toContain('32 ms');
  });
});

describe('命令面板过滤', () => {
  it('空查询保留全部条目', () => {
    const entries = buildPalette(input());
    expect(filterPalette(entries, '  ')).toHaveLength(entries.length);
  });

  it('按标题与 host 子序列模糊匹配', () => {
    const entries = buildPalette(input());
    expect(filterPalette(entries, '迁移').map((entry) => entry.label)).toContain('迁移与恢复');
    expect(filterPalette(entries, 'studio').map((entry) => entry.label)).toContain(
      '类型生成一致性',
    );
    expect(
      filterPalette(entries, 'mba').map((entry) => entry.action),
    ).toContainEqual({ kind: 'host', host: 'macbook-air-01' });
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
