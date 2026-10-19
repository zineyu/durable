import type { BootstrapResponse } from '../api/generated/BootstrapResponse';
import type { FactEvent } from '../api/generated/FactEvent';
import type { SessionMetadata } from '../api/generated/SessionMetadata';
import type { WorkspaceMetadata } from '../api/generated/WorkspaceMetadata';
import { toSessionId, toWorkspaceId, toWorkspacePath } from './ids';
import type { SessionSummary, WorkspaceSummary } from './types';

export type SidebarState = {
  modelName: string;
  workspaces: WorkspaceMetadata[];
  sessions: SessionMetadata[];
};

export function createSidebarState(bootstrap: BootstrapResponse): SidebarState {
  return {
    modelName: bootstrap.model,
    workspaces: bootstrap.workspaces,
    sessions: bootstrap.sessions,
  };
}

export function applyEvent(state: SidebarState, event: FactEvent): SidebarState {
  switch (event.kind) {
    case 'workspace_registered':
      return {
        ...state,
        workspaces: state.workspaces.some((workspace) => workspace.path === event.workspace.path)
          ? state.workspaces
          : [...state.workspaces, event.workspace],
      };
    case 'workspace_removed':
      return {
        ...state,
        workspaces: state.workspaces.filter((workspace) => workspace.path !== event.path),
        sessions: state.sessions.filter((session) => session.workspace !== event.path),
      };
    case 'session_created':
    case 'session_renamed':
    case 'message_appended':
    case 'message_updated':
    case 'message_queued':
    case 'run_started':
    case 'run_finished':
      return { ...state, sessions: upsertSession(state.sessions, event.session) };
    case 'queue_item_removed':
      return {
        ...state,
        sessions: state.sessions.map((session) =>
          session.id === event.sessionId
            ? { ...session, queueLength: session.queueLength - 1 }
            : session,
        ),
      };
    case 'session_deleted':
      return {
        ...state,
        sessions: state.sessions.filter((session) => session.id !== event.sessionId),
      };
    default: {
      const unhandled: never = event;
      throw new Error(`未处理的事件: ${JSON.stringify(unhandled)}`);
    }
  }
}

export function sidebarWorkspaces(state: SidebarState): WorkspaceSummary[] {
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
  };
}

function upsertSession(
  sessions: SessionMetadata[],
  incoming: SessionMetadata,
): SessionMetadata[] {
  const index = sessions.findIndex((session) => session.id === incoming.id);
  if (index === -1) {
    return [...sessions, incoming];
  }
  return sessions.map((session, position) => (position === index ? incoming : session));
}
