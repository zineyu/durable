import type { NodeName } from '../api/generated/NodeName';
import { nodeStatusLabel } from './labels';
import type { SessionId } from './ids';
import type { NodeRecord, PaletteSession, SessionSummary } from './types';

/// 面板条目的执行语义。组件只按 `kind` 分派，不解析文本。
export type PaletteAction =
  | { kind: 'session'; node: NodeName; sessionId: SessionId }
  | { kind: 'node'; node: NodeName }
  | { kind: 'register-node' }
  | { kind: 'register-workspace' }
  | { kind: 'cancel-run'; node: NodeName; sessionId: SessionId };

export type PaletteGroup = 'session' | 'action' | 'node';

export type PaletteEntry = {
  id: string;
  group: PaletteGroup;
  label: string;
  detail: string;
  action: PaletteAction;
};

export type PaletteInput = {
  nodes: NodeRecord[];
  /// 跨节点的 session 列表，由派生层给出。
  sessions: PaletteSession[];
  selectedNode: NodeName | null;
  /// 选中 session 的摘要；未选中 session 时为 `null`。
  selectedSession: SessionSummary | null;
};

const groupOrder: PaletteGroup[] = ['session', 'action', 'node'];

/// 组装面板条目：跨节点的 session、动作与节点。
export function buildPalette(input: PaletteInput): PaletteEntry[] {
  const sessions = [...input.sessions].sort(
    (left, right) => right.updatedAt - left.updatedAt,
  );
  return [
    ...sessions.map((session) => ({
      id: `session:${session.node}:${session.sessionId}`,
      group: 'session' as const,
      label: session.title,
      detail: `${session.node} · ${session.workspace}`,
      action: { kind: 'session' as const, node: session.node, sessionId: session.sessionId },
    })),
    ...actions(input),
    ...input.nodes.map((record) => ({
      id: `node:${record.name}`,
      group: 'node' as const,
      label: record.name,
      detail:
        record.detail === null
          ? nodeStatusLabel[record.status]
          : `${nodeStatusLabel[record.status]} · ${record.detail}`,
      action: { kind: 'node' as const, node: record.name },
    })),
  ];
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

function actions(input: PaletteInput): PaletteEntry[] {
  const entries: PaletteEntry[] = [
    {
      id: 'action:register-node',
      group: 'action',
      label: '注册节点',
      detail: '把一台机器加入节点表',
      action: { kind: 'register-node' },
    },
  ];
  const node = input.selectedNode;
  if (node === null) {
    return entries;
  }
  const record = input.nodes.find((entry) => entry.name === node);
  if (record?.status !== 'ok') {
    return entries;
  }
  entries.push({
    id: 'action:register-workspace',
    group: 'action',
    label: '注册 workspace',
    detail: `在 ${node} 上注册目录`,
    action: { kind: 'register-workspace' },
  });
  const session = input.selectedSession;
  if (session?.state === 'running') {
    entries.push({
      id: `action:cancel-run:${node}:${session.id}`,
      group: 'action',
      label: '停止 run',
      detail: `${session.title} · ${node}`,
      action: { kind: 'cancel-run', node, sessionId: session.id },
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
