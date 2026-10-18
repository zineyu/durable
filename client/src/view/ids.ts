declare const brand: unique symbol;

export type WorkspaceId = string & { readonly [brand]: 'WorkspaceId' };
export type WorkspacePath = string & { readonly [brand]: 'WorkspacePath' };
export type SessionId = string & { readonly [brand]: 'SessionId' };
export type QueueItemId = string & { readonly [brand]: 'QueueItemId' };

export function toWorkspaceId(value: string): WorkspaceId {
  if (value.trim().length === 0) {
    throw new Error('WorkspaceId 不能为空');
  }
  return value as WorkspaceId;
}

export function toWorkspacePath(value: string): WorkspacePath {
  if (!value.startsWith('/')) {
    throw new Error('WorkspacePath 必须是绝对路径');
  }
  return value as WorkspacePath;
}

export function toSessionId(value: string): SessionId {
  if (value.trim().length === 0) {
    throw new Error('SessionId 不能为空');
  }
  return value as SessionId;
}

export function toQueueItemId(value: string): QueueItemId {
  if (value.trim().length === 0) {
    throw new Error('QueueItemId 不能为空');
  }
  return value as QueueItemId;
}
