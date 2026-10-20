import type { BootstrapResponse } from './generated/BootstrapResponse';
import type { CreateSessionRequest } from './generated/CreateSessionRequest';
import type { EditMessageRequest } from './generated/EditMessageRequest';
import type { ErrorCode } from './generated/ErrorCode';
import type { ErrorResponse } from './generated/ErrorResponse';
import type { HubBootstrap } from './generated/HubBootstrap';
import type { HubErrorCode } from './generated/HubErrorCode';
import type { HubErrorResponse } from './generated/HubErrorResponse';
import type { MessageNode } from './generated/MessageNode';
import type { NodeName } from './generated/NodeName';
import type { NodeSnapshot } from './generated/NodeSnapshot';
import type { QueueItem } from './generated/QueueItem';
import type { RegisterNodeRequest } from './generated/RegisterNodeRequest';
import type { RegisterWorkspaceRequest } from './generated/RegisterWorkspaceRequest';
import type { RenameNodeRequest } from './generated/RenameNodeRequest';
import type { RemoveWorkspaceRequest } from './generated/RemoveWorkspaceRequest';
import type { RenameSessionRequest } from './generated/RenameSessionRequest';
import type { SessionMetadata } from './generated/SessionMetadata';
import type { SessionTree } from './generated/SessionTree';
import type { SubmitMessageRequest } from './generated/SubmitMessageRequest';
import type { SwitchActiveLeafRequest } from './generated/SwitchActiveLeafRequest';
import type { WorkspaceMetadata } from './generated/WorkspaceMetadata';

/// hub 或节点拒绝请求、或 socket 失败时的失败类别。
export type ApiFailure = {
  code: ErrorCode | HubErrorCode | 'network';
  message: string;
};

export type ApiResult<T> = { ok: true; value: T } | { ok: false; error: ApiFailure };

type FailureBody = ErrorResponse | HubErrorResponse;

async function request<T>(method: string, path: string, body?: unknown): Promise<ApiResult<T>> {
  const init: RequestInit = { method };
  if (body !== undefined) {
    init.headers = { 'content-type': 'application/json' };
    init.body = JSON.stringify(body);
  }
  const response = await fetch(path, init).catch(() => null);
  if (response === null) {
    return { ok: false, error: { code: 'network', message: `请求 hub 失败: ${method} ${path}` } };
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
  const body = (await response.json().catch(() => null)) as FailureBody | null;
  if (body === null) {
    return { code: 'network', message: `hub 返回 ${response.status}` };
  }
  return { code: body.code, message: body.message };
}

/// 节点作用域请求。hub 剥掉前缀后原样转发给该节点。
function nodePath(node: NodeName, rest: string): string {
  return `/api/n/${encodeURIComponent(node)}/${rest}`;
}

function sessionPath(node: NodeName, id: string): string {
  return nodePath(node, `sessions/${encodeURIComponent(id)}`);
}

export const api = {
  bootstrap(): Promise<ApiResult<HubBootstrap>> {
    return request('GET', '/api/bootstrap');
  },
  nodeBootstrap(node: NodeName): Promise<ApiResult<BootstrapResponse>> {
    return request('GET', nodePath(node, 'bootstrap'));
  },
  registerNode(name: string, url: string): Promise<ApiResult<NodeSnapshot>> {
    const body: RegisterNodeRequest = { name, url };
    return request('POST', '/api/nodes', body);
  },
  renameNode(name: NodeName, next: string): Promise<ApiResult<NodeSnapshot>> {
    const body: RenameNodeRequest = { name: next };
    return request('PATCH', `/api/nodes/${encodeURIComponent(name)}`, body);
  },
  removeNode(name: NodeName): Promise<ApiResult<void>> {
    return request('DELETE', `/api/nodes/${encodeURIComponent(name)}`);
  },
  sessionTree(node: NodeName, id: string): Promise<ApiResult<SessionTree>> {
    return request('GET', `${sessionPath(node, id)}/tree`);
  },
  registerWorkspace(node: NodeName, path: string): Promise<ApiResult<WorkspaceMetadata>> {
    const body: RegisterWorkspaceRequest = { path };
    return request('POST', nodePath(node, 'workspaces'), body);
  },
  removeWorkspace(node: NodeName, path: string): Promise<ApiResult<void>> {
    const body: RemoveWorkspaceRequest = { path };
    return request('POST', nodePath(node, 'workspaces/remove'), body);
  },
  createSession(
    node: NodeName,
    workspace: string,
    title: string | null,
  ): Promise<ApiResult<SessionMetadata>> {
    const body: CreateSessionRequest = { workspace, title };
    return request('POST', nodePath(node, 'sessions'), body);
  },
  renameSession(node: NodeName, id: string, title: string): Promise<ApiResult<SessionMetadata>> {
    const body: RenameSessionRequest = { title };
    return request('POST', `${sessionPath(node, id)}/rename`, body);
  },
  deleteSession(node: NodeName, id: string): Promise<ApiResult<void>> {
    return request('POST', `${sessionPath(node, id)}/delete`);
  },
  submitMessage(node: NodeName, id: string, text: string): Promise<ApiResult<QueueItem>> {
    const body: SubmitMessageRequest = { text };
    return request('POST', `${sessionPath(node, id)}/messages`, body);
  },
  editUserMessage(
    node: NodeName,
    id: string,
    messageId: number,
    text: string,
  ): Promise<ApiResult<MessageNode>> {
    const body: EditMessageRequest = { text };
    return request('POST', `${sessionPath(node, id)}/messages/${messageId}/edit`, body);
  },
  retryAssistantMessage(
    node: NodeName,
    id: string,
    messageId: number,
  ): Promise<ApiResult<void>> {
    return request('POST', `${sessionPath(node, id)}/messages/${messageId}/retry`);
  },
  switchActiveLeaf(
    node: NodeName,
    id: string,
    messageId: number,
  ): Promise<ApiResult<void>> {
    const body: SwitchActiveLeafRequest = { messageId };
    return request('POST', `${sessionPath(node, id)}/active-leaf`, body);
  },
  removeQueueItem(
    node: NodeName,
    id: string,
    itemId: number,
  ): Promise<ApiResult<void>> {
    return request('POST', `${sessionPath(node, id)}/queue/${itemId}/remove`);
  },
  cancelRun(node: NodeName, id: string): Promise<ApiResult<void>> {
    return request('POST', `${sessionPath(node, id)}/cancel`);
  },
};
