import type { NodeName } from '../api/generated/NodeName';
import type { SessionMetadata } from '../api/generated/SessionMetadata';
import { nodeStatusLabel } from './labels';
import { toSessionId } from './ids';
import type { SessionId } from './ids';
import type { NodeState } from './node-state';
import type { NodeRecord } from './types';

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
  shards: Record<NodeName, NodeState>;
  selectedNode: NodeName | null;
  selectedSessionId: SessionId | null;
};

const groupOrder: PaletteGroup[] = ['session', 'action', 'node'];

/// 组装面板条目：跨节点的 session、动作与节点。
export function buildPalette(input: PaletteInput): PaletteEntry[] {
  const sessions = allSessions(input.nodes, input.shards).sort(
    (left, right) => right.session.updatedAt - left.session.updatedAt,
  );
  return [
    ...sessions.map(({ node, session }) => ({
      id: `session:${node}:${session.id}`,
      group: 'session' as const,
      label: session.title,
      detail: `${node} · ${session.workspace}`,
      action: { kind: 'session' as const, node, sessionId: toSessionId(session.id) },
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

function allSessions(
  nodes: NodeRecord[],
  shards: Record<NodeName, NodeState>,
): { node: NodeName; session: SessionMetadata }[] {
  const sessions: { node: NodeName; session: SessionMetadata }[] = [];
  for (const record of nodes) {
    const shard = shards[record.name];
    if (shard === undefined) {
      continue;
    }
    for (const session of shard.sessions) {
      sessions.push({ node: record.name, session });
    }
  }
  return sessions;
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
  const shard = input.shards[node];
  const session =
    shard === undefined || input.selectedSessionId === null
      ? undefined
      : shard.sessions.find((entry) => entry.id === input.selectedSessionId);
  if (session?.busy === true) {
    entries.push({
      id: `action:cancel-run:${node}:${session.id}`,
      group: 'action',
      label: '停止 run',
      detail: `${session.title} · ${node}`,
      action: { kind: 'cancel-run', node, sessionId: toSessionId(session.id) },
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
