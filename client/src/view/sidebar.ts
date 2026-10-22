import type { SessionMetadata } from '../api/generated/SessionMetadata';
import type { WorkspaceMetadata } from '../api/generated/WorkspaceMetadata';
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
      .map(toSessionSummary),
  }));
}

function compareWorkspaces(left: WorkspaceMetadata, right: WorkspaceMetadata): number {
  return left.registeredAt - right.registeredAt || left.path.localeCompare(right.path);
}

function compareSessions(left: SessionMetadata, right: SessionMetadata): number {
  return right.updatedAt - left.updatedAt;
}

function toSessionSummary(session: SessionMetadata): SessionSummary {
  const state = session.busy ? 'running' : session.queueLength > 0 ? 'queued' : 'idle';
  return {
    id: toSessionId(session.id),
    title: session.title,
    state,
    queuedCount: session.queueLength,
    updatedAt: session.updatedAt,
  };
}
