import type { MessageSkeleton } from '../api/generated/MessageSkeleton';
import type { NodeName } from '../api/generated/NodeName';
import type { RunRecord } from '../api/generated/RunRecord';
import type { SessionMetadata } from '../api/generated/SessionMetadata';
import type { ToolResultState } from '../api/generated/ToolResultState';
import type { WorkspaceMetadata } from '../api/generated/WorkspaceMetadata';
import { bodyText } from './content';
import { toQueueItemId, toSessionId, toWorkspaceId, toWorkspacePath } from './ids';
import { retryNote, runFailureLabel, sessionStateOf } from './labels';
import type { NodeState, SessionTreeState } from './node-state';
import type {
  BranchPosition,
  MessageState,
  NodeRecord,
  NodeSummary,
  PaletteSession,
  QueueItem,
  RetryState,
  RunningEntry,
  SessionOutcome,
  SessionSummary,
  SessionView,
  TranscriptEntry,
  WorkspaceSummary,
} from './types';

/// 本模块负责由节点分片推导呈现所需的视图：session 摘要、完整 session 视图、
/// workspace 分组、节点摘要、运行中列表、命令面板 session 列表与缺失分片的节点名。
/// 它是分片的唯一常规读取者：其它模块不得为呈现目的读取分片字段，
/// 也不得遍历 `Record<NodeName, NodeState>`。
/// 不属于它的：事实归约在 `reducer.ts`，块正文缓存的状态机在 `bodies.ts`，
/// 组件渲染在 `components/`。

export function sessionSummaries(shard: NodeState): SessionSummary[] {
  return rowsOf(shard).map((row) => row.summary);
}

export function workspaceSummaries(shard: NodeState): WorkspaceSummary[] {
  const rows = rowsOf(shard);
  return [...shard.workspaces].sort(compareWorkspaces).map((workspace) => ({
    id: toWorkspaceId(workspace.path),
    path: toWorkspacePath(workspace.path),
    sessions: rows
      .filter((row) => row.session.workspace === workspace.path)
      .sort((left, right) => right.session.updatedAt - left.session.updatedAt)
      .map((row) => row.summary),
  }));
}

export function nodeSummary(shard: NodeState): NodeSummary {
  return { modelName: shard.modelName, workspaces: workspaceSummaries(shard) };
}

/// 侧栏展示顺序下的全部 session，供快捷键在同一顺序里切换。
export function orderedSessions(workspaces: WorkspaceSummary[]): SessionSummary[] {
  return workspaces.flatMap((workspace) => workspace.sessions);
}

export function sessionView(shard: NodeState, sessionId: string): SessionView | null {
  const session = shard.sessions.find((entry) => entry.id === sessionId);
  if (session === undefined) {
    return null;
  }
  const transcript = transcriptEntries(shard, sessionId);
  return {
    session,
    summary: summaryOf(shard, session),
    transcript,
    history: sessionHistory(transcript),
    toolResults: toolResultsOf(shard, sessionId),
    queue: queueOf(shard, sessionId),
    placeholders: shard.placeholders[sessionId] ?? [],
    lastRun: latestRun(shard, sessionId)?.status ?? null,
    idle: !session.busy && session.queueLength === 0,
  };
}

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

/// 跨节点的 session 列表，供命令面板使用。
export function paletteSessions(
  nodes: NodeRecord[],
  shards: Record<NodeName, NodeState>,
): PaletteSession[] {
  const sessions: PaletteSession[] = [];
  for (const record of nodes) {
    const shard = shards[record.name];
    if (shard === undefined) {
      continue;
    }
    for (const session of shard.sessions) {
      sessions.push({
        node: record.name,
        sessionId: toSessionId(session.id),
        title: session.title,
        workspace: session.workspace,
        updatedAt: session.updatedAt,
      });
    }
  }
  return sessions;
}

/// `ok` 但尚无分片的节点名。调用方据此取节点作用域的 bootstrap。
export function missingShards(
  nodes: NodeRecord[],
  shards: Record<NodeName, NodeState>,
): NodeName[] {
  return nodes
    .filter((record) => record.status === 'ok' && shards[record.name] === undefined)
    .map((record) => record.name);
}

/// 活动链上尚未取得正文、也没有在请求中的消息 id。调用方据此批量拉取。
export function missingBodyIds(shard: NodeState, sessionId: string): number[] {
  const tree = shard.trees[sessionId];
  if (tree === undefined) {
    return [];
  }
  return chainOf(tree)
    .map((message) => message.id)
    .filter(
      (id) =>
        shard.bodies[id] === undefined &&
        !shard.bodyRequests.includes(id) &&
        !shard.bodyFailures.includes(id),
    );
}

/// 输入框回溯用的用户消息正文，按链上顺序。正文未加载或为空的消息不计入。
export function sessionHistory(transcript: TranscriptEntry[]): string[] {
  const history: string[] = [];
  for (const entry of transcript) {
    if (entry.message.role !== 'user' || entry.parts === null) {
      continue;
    }
    const text = bodyText(entry.parts);
    if (text.trim().length > 0) {
      history.push(text);
    }
  }
  return history;
}

function rowsOf(
  shard: NodeState,
): { session: SessionMetadata; summary: SessionSummary }[] {
  return shard.sessions.map((session) => ({ session, summary: summaryOf(shard, session) }));
}

function summaryOf(shard: NodeState, session: SessionMetadata): SessionSummary {
  return {
    id: toSessionId(session.id),
    title: session.title,
    state: sessionStateOf(session.busy, session.queueLength),
    queuedCount: session.queueLength,
    updatedAt: session.updatedAt,
    unread: session.updatedAt > (shard.lastSeenAt[session.id] ?? 0),
    outcome: outcomeOf(latestRun(shard, session.id)),
  };
}

/// 最近一次 run，按更新时间取最新。
function latestRun(shard: NodeState, sessionId: string): RunRecord | null {
  let latest: RunRecord | null = null;
  for (const run of shard.trees[sessionId]?.runs ?? []) {
    if (latest === null || run.updatedAt > latest.updatedAt) {
      latest = run;
    }
  }
  return latest;
}

/// 结局只在最近的 run 成功或失败时给出。
function outcomeOf(run: RunRecord | null): SessionOutcome | null {
  if (run?.status === 'succeeded' || run?.status === 'failed') {
    return run.status;
  }
  return null;
}

/// root 到 active leaf 的完整链。工具节点也在链上。
function chainOf(tree: SessionTreeState): MessageSkeleton[] {
  if (tree.activeLeaf === null) {
    return [];
  }
  const byId = new Map(tree.nodes.map((node) => [node.id, node]));
  const chain: MessageSkeleton[] = [];
  let current = byId.get(tree.activeLeaf);
  while (current !== undefined) {
    chain.push(current);
    if (current.parentId === null) {
      break;
    }
    current = byId.get(current.parentId);
  }
  return chain.reverse();
}

/// 活动链上供渲染的消息。工具节点的正文只经 `toolResults` 呈现。
function transcriptEntries(shard: NodeState, sessionId: string): TranscriptEntry[] {
  const tree = shard.trees[sessionId];
  if (tree === undefined) {
    return [];
  }
  const siblings = siblingIndex(tree.nodes);
  return chainOf(tree)
    .filter((message) => message.role !== 'tool')
    .map((message) => {
      const display = messageDisplay(tree.runs, shard.retries[sessionId], message);
      return {
        message,
        parts: shard.bodies[message.id] ?? null,
        state: display.state,
        note: display.note,
        branch: branchAt(siblings, message),
      };
    });
}

/// 按父节点分组的兄弟表，每组按 id 升序。工具节点与压缩节点不参与分支。
function siblingIndex(nodes: MessageSkeleton[]): Map<number | null, MessageSkeleton[]> {
  const groups = new Map<number | null, MessageSkeleton[]>();
  for (const node of nodes) {
    if (node.role === 'tool' || node.role === 'compaction') {
      continue;
    }
    const group = groups.get(node.parentId);
    if (group === undefined) {
      groups.set(node.parentId, [node]);
    } else {
      group.push(node);
    }
  }
  for (const group of groups.values()) {
    group.sort((left, right) => left.id - right.id);
  }
  return groups;
}

function branchAt(
  siblings: Map<number | null, MessageSkeleton[]>,
  message: MessageSkeleton,
): BranchPosition | null {
  if (message.role === 'compaction') {
    return null;
  }
  const group = siblings.get(message.parentId) ?? [];
  if (group.length < 2) {
    return null;
  }
  const index = group.findIndex((candidate) => candidate.id === message.id);
  return {
    index: index + 1,
    total: group.length,
    previousId: group[index - 1]?.id ?? null,
    nextId: group[index + 1]?.id ?? null,
  };
}

/// 消息的展示状态与说明。说明只在进行中或失败时给出。
function messageDisplay(
  runs: RunRecord[],
  retry: RetryState | undefined,
  message: MessageSkeleton,
): { state: MessageState; note: string | null } {
  if (message.runId === null) {
    return { state: 'complete', note: null };
  }
  const run = runs.find((candidate) => candidate.id === message.runId);
  if (run === undefined) {
    return { state: 'complete', note: null };
  }
  switch (run.status) {
    case 'running':
      return { state: 'streaming', note: retry === undefined ? null : retryNote(retry) };
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

/// 按 call id 汇总工具结局。正文未到时没有条目，调用方按执行中渲染。
function toolResultsOf(shard: NodeState, sessionId: string): Record<string, ToolResultState> {
  const tree = shard.trees[sessionId];
  if (tree === undefined) {
    return {};
  }
  const results: Record<string, ToolResultState> = {};
  for (const node of tree.nodes) {
    if (node.role !== 'tool') {
      continue;
    }
    for (const part of shard.bodies[node.id] ?? []) {
      if (part.kind === 'tool_result') {
        results[part.callId] = part.state;
      }
    }
  }
  return results;
}

function queueOf(shard: NodeState, sessionId: string): QueueItem[] {
  return (shard.trees[sessionId]?.queue ?? []).map((item) => ({
    id: toQueueItemId(String(item.id)),
    text: item.text,
    pendingSteer: item.pendingSteer,
  }));
}

function compareWorkspaces(left: WorkspaceMetadata, right: WorkspaceMetadata): number {
  return left.registeredAt - right.registeredAt || left.path.localeCompare(right.path);
}
