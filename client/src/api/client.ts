import type { BootstrapResponse } from './generated/BootstrapResponse';
import type { CreateSessionRequest } from './generated/CreateSessionRequest';
import type { EditMessageRequest } from './generated/EditMessageRequest';
import type { ErrorCode } from './generated/ErrorCode';
import type { ErrorResponse } from './generated/ErrorResponse';
import type { HostKind } from './generated/HostKind';
import type { HostName } from './generated/HostName';
import type { HostSnapshot } from './generated/HostSnapshot';
import type { MessageBodiesRequest } from './generated/MessageBodiesRequest';
import type { MessageBodiesResponse } from './generated/MessageBodiesResponse';
import type { MessageSkeleton } from './generated/MessageSkeleton';
import type { QueueItem } from './generated/QueueItem';
import type { RegisterHostRequest } from './generated/RegisterHostRequest';
import type { RegisterWorkspaceRequest } from './generated/RegisterWorkspaceRequest';
import type { RemoveWorkspaceRequest } from './generated/RemoveWorkspaceRequest';
import type { RenameSessionRequest } from './generated/RenameSessionRequest';
import type { SessionMetadata } from './generated/SessionMetadata';
import type { SessionTreeSnapshot } from './generated/SessionTreeSnapshot';
import type { SubmitMessageRequest } from './generated/SubmitMessageRequest';
import type { SubmitMode } from './generated/SubmitMode';
import type { SwitchActiveLeafRequest } from './generated/SwitchActiveLeafRequest';
import type { UpdateHostRequest } from './generated/UpdateHostRequest';
import type { WorkspaceMetadata } from './generated/WorkspaceMetadata';

/// agent 拒绝请求、或 socket 失败时的失败类别。
export type ApiFailure = {
  code: ErrorCode | 'network';
  message: string;
};

export type ApiResult<T> = { ok: true; value: T } | { ok: false; error: ApiFailure };

async function request<T>(
  method: string,
  path: string,
  body?: unknown,
  signal?: AbortSignal,
): Promise<ApiResult<T>> {
  const init: RequestInit = { method };
  if (body !== undefined) {
    init.headers = { 'content-type': 'application/json' };
    init.body = JSON.stringify(body);
  }
  if (signal !== undefined) {
    init.signal = signal;
  }
  const response = await fetch(path, init).catch(() => null);
  if (response === null) {
    return { ok: false, error: { code: 'network', message: `请求 agent 失败: ${method} ${path}` } };
  }
  if (!response.ok) {
    return { ok: false, error: await readFailure(response) };
  }
  if (response.status === 204) {
    return { ok: true, value: undefined as unknown as T };
  }
  const value = (await response.json()) as T;
  return { ok: true, value };
}

async function readFailure(response: Response): Promise<ApiFailure> {
  const body = (await response.json().catch(() => null)) as ErrorResponse | null;
  if (body === null) {
    return { code: 'network', message: `agent 返回 ${response.status}` };
  }
  return { code: body.code, message: body.message };
}

function sessionPath(id: string): string {
  return `/api/sessions/${encodeURIComponent(id)}`;
}

export const api = {
  bootstrap(): Promise<ApiResult<BootstrapResponse>> {
    return request('GET', '/api/bootstrap');
  },
  registerHost(
    name: string,
    kind: HostKind,
    target: string | null,
  ): Promise<ApiResult<HostSnapshot>> {
    const body: RegisterHostRequest = { name, kind, target };
    return request('POST', '/api/hosts', body);
  },
  updateHost(
    name: HostName,
    update: { name?: string | null; target?: string | null; enabled?: boolean | null },
  ): Promise<ApiResult<HostSnapshot>> {
    const body: UpdateHostRequest = {
      name: update.name ?? null,
      target: update.target ?? null,
      enabled: update.enabled ?? null,
    };
    return request('PATCH', `/api/hosts/${encodeURIComponent(name)}`, body);
  },
  removeHost(name: HostName): Promise<ApiResult<void>> {
    return request('DELETE', `/api/hosts/${encodeURIComponent(name)}`);
  },
  reconnectHost(name: HostName): Promise<ApiResult<HostSnapshot>> {
    return request('POST', `/api/hosts/${encodeURIComponent(name)}/reconnect`);
  },
  sessionTree(id: string): Promise<ApiResult<SessionTreeSnapshot>> {
    return request('GET', `${sessionPath(id)}/tree`);
  },
  registerWorkspace(host: HostName, path: string): Promise<ApiResult<WorkspaceMetadata>> {
    const body: RegisterWorkspaceRequest = { host, path };
    return request('POST', '/api/workspaces', body);
  },
  removeWorkspace(host: HostName, path: string): Promise<ApiResult<void>> {
    const body: RemoveWorkspaceRequest = { host, path };
    return request('POST', '/api/workspaces/remove', body);
  },
  createSession(
    host: HostName,
    workspace: string,
    title: string | null,
  ): Promise<ApiResult<SessionMetadata>> {
    const body: CreateSessionRequest = { host, workspace, title };
    return request('POST', '/api/sessions', body);
  },
  renameSession(id: string, title: string): Promise<ApiResult<SessionMetadata>> {
    const body: RenameSessionRequest = { title };
    return request('POST', `${sessionPath(id)}/rename`, body);
  },
  deleteSession(id: string): Promise<ApiResult<void>> {
    return request('POST', `${sessionPath(id)}/delete`);
  },
  submitMessage(id: string, text: string, mode: SubmitMode): Promise<ApiResult<QueueItem>> {
    const body: SubmitMessageRequest = { text, mode };
    return request('POST', `${sessionPath(id)}/messages`, body);
  },
  editUserMessage(
    id: string,
    messageId: number,
    text: string,
  ): Promise<ApiResult<MessageSkeleton>> {
    const body: EditMessageRequest = { text };
    return request('POST', `${sessionPath(id)}/messages/${messageId}/edit`, body);
  },
  messageBodies(
    id: string,
    messageIds: number[],
    signal?: AbortSignal,
  ): Promise<ApiResult<MessageBodiesResponse>> {
    const body: MessageBodiesRequest = { messageIds };
    return request('POST', `${sessionPath(id)}/message-bodies`, body, signal);
  },
  retryAssistantMessage(id: string, messageId: number): Promise<ApiResult<void>> {
    return request('POST', `${sessionPath(id)}/messages/${messageId}/retry`);
  },
  switchActiveLeaf(id: string, messageId: number): Promise<ApiResult<void>> {
    const body: SwitchActiveLeafRequest = { messageId };
    return request('POST', `${sessionPath(id)}/active-leaf`, body);
  },
  removeQueueItem(id: string, itemId: number): Promise<ApiResult<void>> {
    return request('POST', `${sessionPath(id)}/queue/${itemId}/remove`);
  },
  cancelRun(id: string): Promise<ApiResult<void>> {
    return request('POST', `${sessionPath(id)}/cancel`);
  },
};
