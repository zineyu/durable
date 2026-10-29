import type { HostName } from '../api/generated/HostName';
import { hostStatusLabel } from './labels';
import type { HostRecord, PaletteSession, SessionSummary } from './types';
import type { SessionId } from './ids';

/// 面板条目的执行语义。组件只按 `kind` 分派，不解析文本。
export type PaletteAction =
  | { kind: 'session'; sessionId: SessionId }
  | { kind: 'host'; host: HostName }
  | { kind: 'register-host' }
  | { kind: 'register-workspace' }
  | { kind: 'cancel-run'; sessionId: SessionId };

export type PaletteGroup = 'session' | 'action' | 'host';

export type PaletteEntry = {
  id: string;
  group: PaletteGroup;
  /// 条目所在的分组标题。session 用 host 名，动作与 host 组用固定标题。
  heading: string;
  label: string;
  detail: string;
  action: PaletteAction;
};

export type PaletteInput = {
  hosts: HostRecord[];
  /// 跨 host 的 session 列表，由派生层给出。
  sessions: PaletteSession[];
  selectedHost: HostName | null;
  /// 选中 session 的摘要；未选中 session 时为 `null`。
  selectedSession: SessionSummary | null;
};

const groupOrder: PaletteGroup[] = ['session', 'action', 'host'];

/// 组装面板条目：按 host 分组的 session、动作与 host 列表。
/// 规格 user story R-48：命令面板按 host 分组。
export function buildPalette(input: PaletteInput): PaletteEntry[] {
  return [...sessionsOf(input), ...actions(input), ...hostEntries(input)];
}

/// 模糊过滤。空查询保持原顺序；组内按匹配度排序，组顺序不变。
export function filterPalette(entries: PaletteEntry[], query: string): PaletteEntry[] {
  const needle = query.trim();
  if (needle.length === 0) {
    return entries;
  }
  return groupOrder.flatMap((group) =>
    entries
      .map((entry, order) => ({ entry, order }))
      .filter((candidate) => candidate.entry.group === group)
      .map((candidate) => ({
        ...candidate,
        score: fuzzyScore(needle, `${candidate.entry.label} ${candidate.entry.detail}`),
      }))
      .filter((candidate): candidate is typeof candidate & { score: number } => candidate.score !== null)
      .sort((left, right) => right.score - left.score || left.order - right.order)
      .map((candidate) => candidate.entry),
  );
}

export function stepPaletteIndex(current: number, delta: number, total: number): number {
  if (total <= 0) {
    return 0;
  }
  return (current + delta + total) % total;
}

/// session 按 host 分组：host 顺序沿用 host 表，组内最近更新的在前。
function sessionsOf(input: PaletteInput): PaletteEntry[] {
  const order = new Map(input.hosts.map((host, index) => [host.name, index]));
  return [...input.sessions]
    .sort(
      (left, right) =>
        (order.get(left.host) ?? Number.MAX_SAFE_INTEGER) -
          (order.get(right.host) ?? Number.MAX_SAFE_INTEGER) ||
        right.updatedAt - left.updatedAt,
    )
    .map((session) => ({
      id: `session:${session.host}:${session.sessionId}`,
      group: 'session' as const,
      heading: session.host,
      label: session.title,
      detail: `${session.host} · ${session.workspace}`,
      action: { kind: 'session' as const, sessionId: session.sessionId },
    }));
}

function hostEntries(input: PaletteInput): PaletteEntry[] {
  return input.hosts.map((record) => ({
    id: `host:${record.name}`,
    group: 'host' as const,
    heading: 'host',
    label: record.name,
    detail: hostDetail(record),
    action: { kind: 'host' as const, host: record.name },
  }));
}

function hostDetail(record: HostRecord): string {
  const status =
    record.detail === null
      ? hostStatusLabel[record.status]
      : `${hostStatusLabel[record.status]} · ${record.detail}`;
  return record.latencyMs === null ? status : `${status} · ${record.latencyMs} ms`;
}

function actions(input: PaletteInput): PaletteEntry[] {
  const entries: PaletteEntry[] = [
    {
      id: 'action:register-host',
      group: 'action',
      heading: '动作',
      label: '注册 host',
      detail: '把一台机器加入 host 表',
      action: { kind: 'register-host' },
    },
  ];
  const host = input.selectedHost;
  if (host === null) {
    return entries;
  }
  const record = input.hosts.find((entry) => entry.name === host);
  if (record?.status !== 'ok') {
    return entries;
  }
  entries.push({
    id: 'action:register-workspace',
    group: 'action',
    heading: '动作',
    label: '注册 workspace',
    detail: `在 ${host} 上注册目录`,
    action: { kind: 'register-workspace' },
  });
  const session = input.selectedSession;
  if (session?.state === 'running') {
    entries.push({
      id: `action:cancel-run:${session.id}`,
      group: 'action',
      heading: '动作',
      label: '停止 run',
      detail: `${session.title} · ${host}`,
      action: { kind: 'cancel-run', sessionId: session.id },
    });
  }
  return entries;
}

/// 子序列匹配。连续命中与词首命中得分更高。
function fuzzyScore(query: string, text: string): number | null {
  const needle = query.toLowerCase();
  const haystack = text.toLowerCase();
  let score = 0;
  let position = 0;
  let previous = -2;
  for (const character of needle) {
    const found = haystack.indexOf(character, position);
    if (found === -1) {
      return null;
    }
    score += found === previous + 1 ? 3 : 1;
    if (found === 0 || isBoundary(haystack.charAt(found - 1))) {
      score += 2;
    }
    previous = found;
    position = found + 1;
  }
  return score;
}

function isBoundary(character: string): boolean {
  return /[\s·/_-]/.test(character);
}
