import type { MessageSkeleton } from '../api/generated/MessageSkeleton';
import type { QueueItem } from '../api/generated/QueueItem';
import { retryNote, runFailureLabel } from './labels';
import type { MessageState } from './types';
import type { NodeState } from './node-state';

/// root 到 active leaf 的完整路径。工具节点也在链上，渲染时另按角色过滤。
export function transcriptNodes(state: NodeState, sessionId: string): MessageSkeleton[] {
  const tree = state.trees[sessionId];
  if (tree === undefined || tree.activeLeaf === null) {
    return [];
  }
  const byId = new Map(tree.nodes.map((node) => [node.id, node]));
  const path: MessageSkeleton[] = [];
  let current = byId.get(tree.activeLeaf);
  while (current !== undefined) {
    path.push(current);
    if (current.parentId === null) {
      break;
    }
    current = byId.get(current.parentId);
  }
  return path.reverse();
}

export function sessionQueue(state: NodeState, sessionId: string): QueueItem[] {
  return state.trees[sessionId]?.queue ?? [];
}

export function messageView(
  state: NodeState,
  sessionId: string,
  node: MessageSkeleton,
): { state: MessageState; note: string | null } {
  if (node.runId === null) {
    return { state: 'complete', note: null };
  }
  const run = state.trees[sessionId]?.runs.find((candidate) => candidate.id === node.runId);
  if (run === undefined) {
    return { state: 'complete', note: null };
  }
  switch (run.status) {
    case 'running': {
      const retry = state.retries[sessionId];
      return { state: 'streaming', note: retry === undefined ? null : retryNote(retry) };
    }
    case 'failed':
      return {
        state: 'failed',
        note:
          run.failure === null
            ? null
            : `${runFailureLabel[run.failure.code]}：${run.failure.message}`,
      };
    case 'cancelled':
      return { state: 'cancelled', note: null };
    case 'succeeded':
      return { state: 'complete', note: null };
    default: {
      const unhandled: never = run.status;
      throw new Error(`未处理的 run 状态: ${JSON.stringify(unhandled)}`);
    }
  }
}

export type BranchPosition = {
  index: number;
  total: number;
  previousId: number | null;
  nextId: number | null;
};

export function branchPosition(
  state: NodeState,
  sessionId: string,
  node: MessageSkeleton,
): BranchPosition | null {
  const nodes = state.trees[sessionId]?.nodes ?? [];
  const siblings = nodes
    .filter(
      (candidate) =>
        candidate.role !== 'tool' &&
        candidate.role !== 'compaction' &&
        candidate.parentId === node.parentId,
    )
    .sort((left, right) => left.id - right.id);
  if (siblings.length < 2) {
    return null;
  }
  const index = siblings.findIndex((candidate) => candidate.id === node.id);
  return {
    index: index + 1,
    total: siblings.length,
    previousId: siblings[index - 1]?.id ?? null,
    nextId: siblings[index + 1]?.id ?? null,
  };
}
