import type { MessageSkeleton } from '../api/generated/MessageSkeleton';
import type { RunRecord } from '../api/generated/RunRecord';
import type { SessionMetadata } from '../api/generated/SessionMetadata';
import type { ToolResultState } from '../api/generated/ToolResultState';
import type { WorkspaceMetadata } from '../api/generated/WorkspaceMetadata';
import { bodyText } from './content';
import { toQueueItemId, toSessionId, toWorkspaceId, toWorkspacePath } from './ids';
import { retryNote, runFailureLabel, sessionStateOf } from './labels';
import type { ClientState, SessionTreeState } from './state';
import type {
  BranchPosition,
  MessageState,
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

/// 本模块负责由唯一一份客户端状态推导呈现所需的视图：session 摘要、完整 session 视图、
/// workspace 分组、运行中列表、命令面板 session 列表与缺失正文 id。
/// 它是状态的唯一常规读取者：其它模块不得为呈现目的读取状态字段。
/// 不属于它的：事实归约在 `state.ts` 与 `reducer.ts`，块正文缓存的状态机在 `bodies.ts`，
/// 组件渲染在 `components/`。

export function sessionSummaries(state: ClientState): SessionSummary[] {
  return summaryRows(state).map((row) => row.summary);
}

export function workspaceSummaries(state: ClientState): WorkspaceSummary[] {
  const rows = summaryRows(state);
  return [...state.workspaces].sort(compareWorkspaces).map((workspace) => ({
    id: toWorkspaceId(workspace.host, workspace.path),
    host: workspace.host,
    path: toWorkspacePath(workspace.path),
    sessions: hostAvailable(state, workspace.host)
      ? rows
          .filter(
            (row) =>
              row.session.host === workspace.host && row.session.workspace === workspace.path,
          )
          .sort((left, right) => right.session.updatedAt - left.session.updatedAt)
          .map((row) => row.summary)
      : [],
  }));
}

/// host 可用时它的 workspace 才列出 session。不可用 host 的列表为空，
/// 侧栏与主面板据 host 快照显示说明（规格 user story R-47）。
export function hostAvailable(state: ClientState, host: string): boolean {
  return state.hosts.some((record) => record.name === host && record.status === 'ok');
}

/// 侧栏展示顺序下的全部 session，供快捷键在同一顺序里切换。
export function orderedSessions(workspaces: WorkspaceSummary[]): SessionSummary[] {
  return workspaces.flatMap((workspace) => workspace.sessions);
}

export function sessionView(state: ClientState, sessionId: string): SessionView | null {
  const session = state.sessions.find((entry) => entry.id === sessionId);
  if (session === undefined) {
    return null;
  }
  const transcript = transcriptEntries(state, sessionId);
  return {
    session,
    summary: summaryOf(state, session),
    transcript,
    history: sessionHistory(transcript),
    toolResults: toolResultsOf(state, sessionId),
    queue: queueOf(state, sessionId),
    placeholders: state.placeholders[sessionId] ?? [],
    lastRun: latestRun(state, sessionId)?.status ?? null,
    idle: !session.busy && session.queueLength === 0,
  };
}

/// 从唯一状态推导全部正在运行的 session。按开始时间从早到晚排列。
export function runningEntries(state: ClientState): RunningEntry[] {
  const entries: RunningEntry[] = [];
  for (const summary of sessionSummaries(state)) {
    if (summary.state !== 'running') {
      continue;
    }
    const running = state.trees[summary.id]?.runs.find((run) => run.status === 'running');
    entries.push({
      host: summary.host,
      sessionId: summary.id,
      title: summary.title,
      queueLength: summary.queuedCount,
      startedAt: running?.createdAt ?? null,
    });
  }
  return entries.sort(
    (left, right) =>
      (left.startedAt ?? Number.MAX_SAFE_INTEGER) - (right.startedAt ?? Number.MAX_SAFE_INTEGER) ||
      left.host.localeCompare(right.host) ||
      left.sessionId.localeCompare(right.sessionId),
  );
}

/// 跨 host 的 session 列表，供命令面板使用。workspace 不在摘要里，因此取整行。
export function paletteSessions(state: ClientState): PaletteSession[] {
  return summaryRows(state).map(({ session, summary }) => ({
    host: session.host,
    sessionId: summary.id,
    title: summary.title,
    workspace: session.workspace,
    updatedAt: summary.updatedAt,
  }));
}

/// 该 session 是否已在状态里。正文拉取与树快照据此决定要不要发请求。
export function hasSession(state: ClientState, sessionId: string): boolean {
  return state.sessions.some((session) => session.id === sessionId);
}

/// 活动链上尚未取得正文、也没有在请求中的消息 id。调用方据此批量拉取。
export function missingBodyIds(state: ClientState, sessionId: string): number[] {
  const tree = state.trees[sessionId];
  if (tree === undefined) {
    return [];
  }
  return chainOf(tree)
    .map((message) => message.id)
    .filter(
      (id) =>
        state.bodies[id] === undefined &&
        !state.bodyRequests.includes(id) &&
        !state.bodyFailures.includes(id),
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

function summaryRows(
  state: ClientState,
): { session: SessionMetadata; summary: SessionSummary }[] {
  return state.sessions.map((session) => ({ session, summary: summaryOf(state, session) }));
}

function summaryOf(state: ClientState, session: SessionMetadata): SessionSummary {
  return {
    id: toSessionId(session.id),
    host: session.host,
    title: session.title,
    state: sessionStateOf(session.busy, session.queueLength),
    queuedCount: session.queueLength,
    updatedAt: session.updatedAt,
    unread: session.updatedAt > (state.lastSeenAt[session.id] ?? 0),
    outcome: outcomeOf(latestRun(state, session.id)),
  };
}

/// 最近一次 run，按更新时间取最新。
function latestRun(state: ClientState, sessionId: string): RunRecord | null {
  let latest: RunRecord | null = null;
  for (const run of state.trees[sessionId]?.runs ?? []) {
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
function transcriptEntries(state: ClientState, sessionId: string): TranscriptEntry[] {
  const tree = state.trees[sessionId];
  if (tree === undefined) {
    return [];
  }
  const siblings = siblingIndex(tree.nodes);
  return chainOf(tree)
    .filter((message) => message.role !== 'tool')
    .map((message) => {
      const display = messageDisplay(tree.runs, state.retries[sessionId], message);
      return {
        message,
        parts: state.bodies[message.id] ?? null,
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
///
/// 已中断标记优先于 run 状态：该消息是崩溃或取消留下的部分输出，后续 run 的结局不改它。
function messageDisplay(
  runs: RunRecord[],
  retry: RetryState | undefined,
  message: MessageSkeleton,
): { state: MessageState; note: string | null } {
  if (message.interrupted) {
    return { state: 'interrupted', note: null };
  }
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
function toolResultsOf(state: ClientState, sessionId: string): Record<string, ToolResultState> {
  const tree = state.trees[sessionId];
  if (tree === undefined) {
    return {};
  }
  const results: Record<string, ToolResultState> = {};
  for (const node of tree.nodes) {
    if (node.role !== 'tool') {
      continue;
    }
    for (const part of state.bodies[node.id] ?? []) {
      if (part.kind === 'tool_result') {
        results[part.callId] = part.state;
      }
    }
  }
  return results;
}

function queueOf(state: ClientState, sessionId: string): QueueItem[] {
  return (state.trees[sessionId]?.queue ?? []).map((item) => ({
    id: toQueueItemId(String(item.id)),
    text: item.text,
    pendingSteer: item.pendingSteer,
  }));
}

function compareWorkspaces(left: WorkspaceMetadata, right: WorkspaceMetadata): number {
  return (
    left.host.localeCompare(right.host) ||
    left.registeredAt - right.registeredAt ||
    left.path.localeCompare(right.path)
  );
}
