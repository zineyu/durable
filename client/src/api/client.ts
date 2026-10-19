import type { BootstrapResponse } from './generated/BootstrapResponse';
import type { CreateSessionRequest } from './generated/CreateSessionRequest';
import type { EditMessageRequest } from './generated/EditMessageRequest';
import type { ErrorCode } from './generated/ErrorCode';
import type { ErrorResponse } from './generated/ErrorResponse';
import type { MessageNode } from './generated/MessageNode';
import type { QueueItem } from './generated/QueueItem';
import type { RegisterWorkspaceRequest } from './generated/RegisterWorkspaceRequest';
import type { RemoveWorkspaceRequest } from './generated/RemoveWorkspaceRequest';
import type { RenameSessionRequest } from './generated/RenameSessionRequest';
import type { SessionMetadata } from './generated/SessionMetadata';
import type { SessionTree } from './generated/SessionTree';
import type { SubmitMessageRequest } from './generated/SubmitMessageRequest';
import type { SwitchActiveLeafRequest } from './generated/SwitchActiveLeafRequest';
import type { WorkspaceMetadata } from './generated/WorkspaceMetadata';

/// agent 拒绝请求或不可达时的失败类别。
export type ApiFailure = {
  code: ErrorCode | 'network';
  message: string;
};

export type ApiResult<T> = { ok: true; value: T } | { ok: false; error: ApiFailure };

async function request<T>(method: string, path: string, body?: unknown): Promise<ApiResult<T>> {
  const init: RequestInit = { method };
  if (body !== undefined) {
    init.headers = { 'content-type': 'application/json' };
    init.body = JSON.stringify(body);
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
  sessionTree(id: string): Promise<ApiResult<SessionTree>> {
    return request('GET', `${sessionPath(id)}/tree`);
  },
  registerWorkspace(path: string): Promise<ApiResult<WorkspaceMetadata>> {
    const body: RegisterWorkspaceRequest = { path };
    return request('POST', '/api/workspaces', body);
  },
  removeWorkspace(path: string): Promise<ApiResult<void>> {
    const body: RemoveWorkspaceRequest = { path };
    return request('POST', '/api/workspaces/remove', body);
  },
  createSession(workspace: string, title: string | null): Promise<ApiResult<SessionMetadata>> {
    const body: CreateSessionRequest = { workspace, title };
    return request('POST', '/api/sessions', body);
  },
  renameSession(id: string, title: string): Promise<ApiResult<SessionMetadata>> {
    const body: RenameSessionRequest = { title };
    return request('POST', `${sessionPath(id)}/rename`, body);
  },
  deleteSession(id: string): Promise<ApiResult<void>> {
    return request('POST', `${sessionPath(id)}/delete`);
  },
  submitMessage(id: string, text: string): Promise<ApiResult<QueueItem>> {
    const body: SubmitMessageRequest = { text };
    return request('POST', `${sessionPath(id)}/messages`, body);
  },
  editUserMessage(id: string, messageId: number, text: string): Promise<ApiResult<MessageNode>> {
    const body: EditMessageRequest = { text };
    return request('POST', `${sessionPath(id)}/messages/${messageId}/edit`, body);
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
