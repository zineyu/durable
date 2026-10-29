import { access, mkdir, mkdtemp, realpath, rm, writeFile } from 'node:fs/promises';
import { constants } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

import type { BootstrapResponse } from '../../src/api/generated/BootstrapResponse';
import type { SessionMetadata } from '../../src/api/generated/SessionMetadata';
import type { SessionTreeSnapshot } from '../../src/api/generated/SessionTreeSnapshot';
import type { WorkspaceMetadata } from '../../src/api/generated/WorkspaceMetadata';
import { ModelServer } from './model-server';
import { launchProcess } from './process';
import type { LaunchedProcess } from './process';

const repositoryRoot = fileURLToPath(new URL('../../../', import.meta.url));
const README = '# durable e2e workspace\n\nhello e2e\n';
const RESTART_ATTEMPTS = 20;
const RESTART_DELAY_MS = 100;

type HarnessInit = {
  root: string;
  workspace: string;
  extraWorkspace: string;
  provider: ModelServer;
  agent: LaunchedProcess;
};

/// 一次端到端场景的完整栈：脚本化模型服务、真实 agent 与临时目录。
/// agent 自己的浏览器入口就是页面地址。
export class Harness {
  readonly root: string;
  readonly workspace: string;
  readonly extraWorkspace: string;
  readonly provider: ModelServer;
  agentUrl: string;
  private agent: LaunchedProcess;
  private readonly database: string;
  private readonly logDirectory: string;

  private constructor(init: HarnessInit) {
    this.root = init.root;
    this.workspace = init.workspace;
    this.extraWorkspace = init.extraWorkspace;
    this.provider = init.provider;
    this.agent = init.agent;
    this.agentUrl = init.agent.url;
    this.database = join(init.root, 'agent.sqlite');
    this.logDirectory = join(init.root, 'logs');
  }

  static async start(): Promise<Harness> {
    const agentBinary = await binaryPath('agent');
    // 解析符号链接，使路径与服务端规范化的 workspace 身份一致。
    const root = await realpath(await mkdtemp(join(tmpdir(), 'durable-e2e-')));
    const workspace = join(root, 'workspace');
    const extraWorkspace = join(root, 'extra');
    await mkdir(workspace);
    await mkdir(extraWorkspace);
    await writeFile(join(workspace, 'README.md'), README);

    const provider = await ModelServer.start();
    const agent = await launchAgent({
      binary: agentBinary,
      listen: '127.0.0.1:0',
      database: join(root, 'agent.sqlite'),
      logDirectory: join(root, 'logs'),
      workspace,
      provider,
    });

    return new Harness({ root, workspace, extraWorkspace, provider, agent });
  }

  /// 浏览器入口。给定 session 时直接打开该 session。
  urlFor(sessionId?: string): string {
    const base = `${this.agentUrl}/`;
    return sessionId === undefined ? base : `${base}#/s/${sessionId}`;
  }

  async registerWorkspace(host: string, path: string): Promise<WorkspaceMetadata> {
    return await this.request<WorkspaceMetadata>('POST', '/api/workspaces', { host, path });
  }

  /// 在指定 host 的 workspace 下创建 session，返回 session id。
  async createSession(workspace: string, host = 'local'): Promise<string> {
    const session = await this.request<SessionMetadata>('POST', '/api/sessions', {
      host,
      workspace,
      title: null,
    });
    return session.id;
  }

  async sessionTree(sessionId: string): Promise<SessionTreeSnapshot> {
    return await this.request<SessionTreeSnapshot>(
      'GET',
      `/api/sessions/${encodeURIComponent(sessionId)}/tree`,
    );
  }

  async bootstrap(): Promise<BootstrapResponse> {
    return await this.request<BootstrapResponse>('GET', '/api/bootstrap');
  }

  /// 用同一端口与同一数据库重启 agent，模拟进程重启与客户端重连。
  async restartAgent(): Promise<void> {
    const listen = new URL(this.agentUrl).host;
    await this.agent.stop();
    this.agent = await this.relaunchAgent(listen);
    this.agentUrl = this.agent.url;
  }

  /// 结束整套栈并删除临时目录。任一步失败都不阻断其余步骤。
  async dispose(): Promise<void> {
    await Promise.allSettled([this.agent.stop(), this.provider.stop()]);
    await rm(this.root, { recursive: true, force: true });
  }

  private async relaunchAgent(listen: string): Promise<LaunchedProcess> {
    const binary = await binaryPath('agent');
    let lastError: unknown = null;
    for (let attempt = 0; attempt < RESTART_ATTEMPTS; attempt += 1) {
      try {
        return await launchAgent({
          binary,
          listen,
          database: this.database,
          logDirectory: this.logDirectory,
          workspace: this.workspace,
          provider: this.provider,
        });
      } catch (error) {
        lastError = error;
        await delay(RESTART_DELAY_MS);
      }
    }
    throw new Error(`agent 重启失败: ${String(lastError)}`);
  }

  private async request<T>(method: string, path: string, body?: unknown): Promise<T> {
    const init: RequestInit = { method };
    if (body !== undefined) {
      init.headers = { 'content-type': 'application/json' };
      init.body = JSON.stringify(body);
    }
    const response = await fetch(`${this.agentUrl}${path}`, init);
    if (!response.ok) {
      throw new Error(`${method} ${path} 失败: ${response.status} ${await response.text()}`);
    }
    if (response.status === 204) {
      return undefined as T;
    }
    return (await response.json()) as T;
  }
}

type AgentLaunch = {
  binary: string;
  listen: string;
  database: string;
  logDirectory: string;
  workspace: string;
  provider: ModelServer;
};

function launchAgent(options: AgentLaunch): Promise<LaunchedProcess> {
  return launchProcess({
    binary: options.binary,
    args: [
      '--listen',
      options.listen,
      '--database',
      options.database,
      '--log-dir',
      options.logDirectory,
      '--workspace',
      `local:${options.workspace}`,
    ],
    environment: {
      ...process.env,
      DURABLE_MODEL_BASE_URL: options.provider.baseUrl,
      DURABLE_MODEL_API_KEY: 'e2e-key',
      DURABLE_MODEL_NAME: 'e2e-model',
    },
    readyLine: 'agent listening on ',
    label: 'agent',
  });
}

async function binaryPath(name: string): Promise<string> {
  // 共享 target 目录时（CARGO_TARGET_DIR）构建产物不在工作区里。
  const target = process.env.CARGO_TARGET_DIR ?? join(repositoryRoot, 'target');
  const path = join(target, 'debug', name);
  try {
    await access(path, constants.X_OK);
  } catch {
    throw new Error(`缺少构建产物 ${path}。先运行 devenv shell -- durable-build。`);
  }
  return path;
}

async function delay(milliseconds: number): Promise<void> {
  await new Promise<void>((resolve) => {
    setTimeout(resolve, milliseconds);
  });
}
