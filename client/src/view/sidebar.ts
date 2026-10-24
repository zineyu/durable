import type { SessionMetadata } from '../api/generated/SessionMetadata';
import type { WorkspaceMetadata } from '../api/generated/WorkspaceMetadata';
import { sessionMark } from './activity';
import { toSessionId, toWorkspaceId, toWorkspacePath } from './ids';
import type { SessionSummary, WorkspaceSummary } from './types';
import type { NodeState } from './node-state';

export function sidebarWorkspaces(state: NodeState): WorkspaceSummary[] {
  const workspaces = [...state.workspaces].sort(compareWorkspaces);
  return workspaces.map((workspace) => ({
    id: toWorkspaceId(workspace.path),
    path: toWorkspacePath(workspace.path),
    sessions: state.sessions
      .filter((session) => session.workspace === workspace.path)
      .sort(compareSessions)
      .map((session) => toSessionSummary(state, session)),
  }));
}

function compareWorkspaces(left: WorkspaceMetadata, right: WorkspaceMetadata): number {
  return left.registeredAt - right.registeredAt || left.path.localeCompare(right.path);
}

function compareSessions(left: SessionMetadata, right: SessionMetadata): number {
  return right.updatedAt - left.updatedAt;
}

function toSessionSummary(state: NodeState, session: SessionMetadata): SessionSummary {
  const mark = sessionMark(state, session.id);
  const status = session.busy ? 'running' : session.queueLength > 0 ? 'queued' : 'idle';
  return {
    id: toSessionId(session.id),
    title: session.title,
    state: status,
    queuedCount: session.queueLength,
    updatedAt: session.updatedAt,
    unread: mark.unread,
    outcome: mark.outcome,
  };
}

/// 侧栏展示顺序下的全部 session，供快捷键在同一顺序里切换。
export function orderedSessions(state: NodeState): SessionSummary[] {
  return sidebarWorkspaces(state).flatMap((workspace) => workspace.sessions);
}
