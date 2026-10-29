import type { HostSnapshot } from '../api/generated/HostSnapshot';
import type { HostMetadata } from '../api/generated/HostMetadata';
import type { SessionMetadata } from '../api/generated/SessionMetadata';
import type { WorkspaceMetadata } from '../api/generated/WorkspaceMetadata';
import { forgetBodies } from './bodies';
import type { ClientState } from './state';
import type { RetryState } from './types';

/// 本模块负责客户端状态的删除与合并助手：session、workspace 与 host 的移除与去重。
/// 事实归约本身在 `state`，派生在 `derived`。

export function removeSession(state: ClientState, sessionId: string): ClientState {
  const removed = forgetBodies(state, messageIds(state, sessionId));
  return {
    ...removed,
    sessions: removed.sessions.filter((session) => session.id !== sessionId),
    trees: withoutKey(removed.trees, sessionId),
    retries: withoutRetryKey(removed.retries, sessionId),
    placeholders: withoutKey(removed.placeholders, sessionId),
    lastSeenAt: withoutKey(removed.lastSeenAt, sessionId),
  };
}

/// 该 session 树里全部消息 id。
export function messageIds(state: ClientState, sessionId: string): number[] {
  return (state.trees[sessionId]?.nodes ?? []).map((node) => node.id);
}

export function removeWorkspace(state: ClientState, host: string, path: string): ClientState {
  const removed = state.sessions
    .filter((session) => session.host === host && session.workspace === path)
    .map((session) => session.id);
  let next = state;
  for (const id of removed) {
    next = removeSession(next, id);
  }
  return {
    ...next,
    workspaces: next.workspaces.filter(
      (workspace) => workspace.host !== host || workspace.path !== path,
    ),
    sessions: next.sessions.filter(
      (session) => session.host !== host || session.workspace !== path,
    ),
  };
}

export function withoutRetryKey(
  retries: Record<string, RetryState>,
  sessionId: string,
): Record<string, RetryState> {
  if (retries[sessionId] === undefined) {
    return retries;
  }
  return Object.fromEntries(Object.entries(retries).filter(([id]) => id !== sessionId));
}

function withoutKey<T>(entries: Record<string, T>, key: string): Record<string, T> {
  return Object.fromEntries(Object.entries(entries).filter(([id]) => id !== key));
}

export function upsertHost(hosts: HostSnapshot[], incoming: HostMetadata): HostSnapshot[] {
  const existing = hosts.find((host) => host.name === incoming.name);
  if (existing === undefined) {
    return [
      ...hosts,
      {
        name: incoming.name,
        kind: incoming.kind,
        target: incoming.target,
        enabled: incoming.enabled,
        status: 'unreachable',
        detail: null,
        latencyMs: null,
      },
    ];
  }
  return hosts.map((host) =>
    host.name === incoming.name
      ? {
          ...host,
          kind: incoming.kind,
          target: incoming.target,
          enabled: incoming.enabled,
        }
      : host,
  );
}

export function upsertWorkspace(
  workspaces: WorkspaceMetadata[],
  incoming: WorkspaceMetadata,
): WorkspaceMetadata[] {
  if (
    workspaces.some(
      (workspace) => workspace.host === incoming.host && workspace.path === incoming.path,
    )
  ) {
    return workspaces;
  }
  return [...workspaces, incoming];
}

export function upsertSession(
  sessions: SessionMetadata[],
  incoming: SessionMetadata,
): SessionMetadata[] {
  if (!sessions.some((session) => session.id === incoming.id)) {
    return [...sessions, incoming];
  }
  return sessions.map((session) => (session.id === incoming.id ? incoming : session));
}
