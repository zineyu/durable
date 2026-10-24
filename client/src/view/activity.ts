import type { NodeName } from '../api/generated/NodeName';
import type { RunRecord } from '../api/generated/RunRecord';
import type { RunStatus } from '../api/generated/RunStatus';
import { toSessionId } from './ids';
import type { SessionId } from './ids';
import type { NodeState } from './node-state';
import type { NodeRecord, SessionOutcome } from './types';

export type RunningEntry = {
  node: NodeName;
  sessionId: SessionId;
  title: string;
  queueLength: number;
  /// 当前 run 的开始时间；分片里还没有 run 记录时为 `null`。
  startedAt: number | null;
};

export type SessionMark = {
  unread: boolean;
  outcome: SessionOutcome | null;
};

/// 从各节点分片推导全部正在运行的 session。按开始时间从早到晚排列。
export function runningEntries(
  nodes: NodeRecord[],
  shards: Record<NodeName, NodeState>,
): RunningEntry[] {
  const entries: RunningEntry[] = [];
  for (const record of nodes) {
    const shard = shards[record.name];
    if (shard === undefined) {
      continue;
    }
    for (const session of shard.sessions) {
      if (!session.busy) {
        continue;
      }
      const running = shard.trees[session.id]?.runs.find((run) => run.status === 'running');
      entries.push({
        node: record.name,
        sessionId: toSessionId(session.id),
        title: session.title,
        queueLength: session.queueLength,
        startedAt: running?.createdAt ?? null,
      });
    }
  }
  return entries.sort(
    (left, right) =>
      (left.startedAt ?? Number.MAX_SAFE_INTEGER) - (right.startedAt ?? Number.MAX_SAFE_INTEGER) ||
      left.node.localeCompare(right.node) ||
      left.sessionId.localeCompare(right.sessionId),
  );
}

/// 最近一次 run，按更新时间取最新。
export function lastRun(shard: NodeState, sessionId: string): RunRecord | null {
  const runs = shard.trees[sessionId]?.runs ?? [];
  let latest: RunRecord | null = null;
  for (const run of runs) {
    if (latest === null || run.updatedAt > latest.updatedAt) {
      latest = run;
    }
  }
  return latest;
}

export function lastRunStatus(shard: NodeState, sessionId: string): RunStatus | null {
  return lastRun(shard, sessionId)?.status ?? null;
}

/// 未读与最近结局。结局只在最近的 run 成功或失败时给出。
export function sessionMark(shard: NodeState, sessionId: string): SessionMark {
  const outcome = sessionOutcome(lastRunStatus(shard, sessionId));
  return { unread: sessionUnread(shard, sessionId), outcome };
}

export function sessionUnread(shard: NodeState, sessionId: string): boolean {
  const session = shard.sessions.find((entry) => entry.id === sessionId);
  if (session === undefined) {
    return false;
  }
  return session.updatedAt > (shard.lastSeenAt[sessionId] ?? 0);
}

function sessionOutcome(status: RunStatus | null): SessionOutcome | null {
  if (status === 'succeeded' || status === 'failed') {
    return status;
  }
  return null;
}
