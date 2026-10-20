import { access, mkdir, mkdtemp, realpath, rm, writeFile } from 'node:fs/promises';
import { constants } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

import type { HubBootstrap } from '../../src/api/generated/HubBootstrap';
import type { SessionMetadata } from '../../src/api/generated/SessionMetadata';
import type { SessionTree } from '../../src/api/generated/SessionTree';
import { ModelServer } from './model-server';
import { launchProcess } from './process';
import type { LaunchedProcess } from './process';

const repositoryRoot = fileURLToPath(new URL('../../../', import.meta.url));
const NODE_NAME = 'e2e-node';
const README = '# durable e2e workspace\n\nhello e2e\n';
const READY_POLL_INTERVAL_MS = 100;
const READY_POLL_ATTEMPTS = 300;
const RESTART_ATTEMPTS = 20;
const RESTART_DELAY_MS = 100;

type HarnessInit = {
  root: string;
  workspace: string;
  extraWorkspace: string;
  provider: ModelServer;
  agent: LaunchedProcess;
  hub: LaunchedProcess;
};

/// 一次端到端场景的完整栈：脚本化模型服务、agent、hub 与临时目录。
export class Harness {
  readonly root: string;
  readonly workspace: string;
  readonly extraWorkspace: string;
  readonly nodeName = NODE_NAME;
  readonly provider: ModelServer;
  hubUrl: string;
  private readonly agent: LaunchedProcess;
  private hub: LaunchedProcess;
  private readonly hubDatabase: string;

  private constructor(init: HarnessInit) {
    this.root = init.root;
    this.workspace = init.workspace;
    this.extraWorkspace = init.extraWorkspace;
    this.provider = init.provider;
    this.agent = init.agent;
    this.hub = init.hub;
    this.hubUrl = init.hub.url;
    this.hubDatabase = join(init.root, 'hub.sqlite');
  }

  static async start(): Promise<Harness> {
    const agentBinary = await binaryPath('agent');
    const hubBinary = await binaryPath('hub');
    // 解析符号链接，使路径与服务端规范化的 workspace 身份一致。
    const root = await realpath(await mkdtemp(join(tmpdir(), 'durable-e2e-')));
    const workspace = join(root, 'workspace');
    const extraWorkspace = join(root, 'extra');
    await mkdir(workspace);
    await mkdir(extraWorkspace);
    await writeFile(join(workspace, 'README.md'), README);

    const provider = await ModelServer.start();
    const agent = await launchProcess({
      binary: agentBinary,
      args: [
        '--listen',
        '127.0.0.1:0',
        '--database',
        join(root, 'agent.sqlite'),
        '--log-dir',
        join(root, 'logs'),
        '--workspace',
        workspace,
      ],
      environment: {
        ...process.env,
        DURABLE_MODEL_BASE_URL: provider.baseUrl,
        DURABLE_MODEL_API_KEY: 'e2e-key',
        DURABLE_MODEL_NAME: 'e2e-model',
      },
      readyLine: 'agent listening on ',
      label: 'agent',
    });
    const hub = await launchProcess({
      binary: hubBinary,
      args: [
        '--listen',
        '127.0.0.1:0',
        '--database',
        join(root, 'hub.sqlite'),
        '--log-dir',
        join(root, 'logs'),
      ],
      environment: process.env,
      readyLine: 'hub listening on ',
      label: 'hub',
    });

    const harness = new Harness({ root, workspace, extraWorkspace, provider, agent, hub });
    try {
      await harness.registerNode();
      return harness;
    } catch (error) {
      await harness.dispose();
      throw error;
    }
  }

  /// 浏览器入口。给定 session 时直接打开该 session。
  urlFor(sessionId?: string): string {
    const base = `${this.hubUrl}/#/n/${this.nodeName}`;
    return sessionId === undefined ? base : `${base}/s/${sessionId}`;
  }

  /// 在指定 workspace 下创建 session，返回 session id。
  async createSession(workspace: string): Promise<string> {
    const session = await this.request<SessionMetadata>('POST', `/api/n/${this.nodeName}/sessions`, {
      workspace,
      title: null,
    });
    return session.id;
  }

  async sessionTree(sessionId: string): Promise<SessionTree> {
    return await this.request<SessionTree>(
      'GET',
      `/api/n/${this.nodeName}/sessions/${encodeURIComponent(sessionId)}/tree`,
    );
  }

  async bootstrap(): Promise<HubBootstrap> {
    return await this.request<HubBootstrap>('GET', '/api/bootstrap');
  }

  /// 用同一端口与同一数据库重启 hub，模拟进程重启与客户端重连。
  async restartHub(): Promise<void> {
    const listen = new URL(this.hub.url).host;
    await this.hub.stop();
    this.hub = await this.relaunchHub(listen);
    this.hubUrl = this.hub.url;
    await this.waitForNode();
  }

  /// 结束整套栈并删除临时目录。任一步失败都不阻断其余步骤。
  async dispose(): Promise<void> {
    await Promise.allSettled([this.hub.stop(), this.agent.stop(), this.provider.stop()]);
    await rm(this.root, { recursive: true, force: true });
  }

  private async relaunchHub(listen: string): Promise<LaunchedProcess> {
    const binary = await binaryPath('hub');
    let lastError: unknown = null;
    for (let attempt = 0; attempt < RESTART_ATTEMPTS; attempt += 1) {
      try {
        return await launchProcess({
          binary,
          args: [
            '--listen',
            listen,
            '--database',
            this.hubDatabase,
            '--log-dir',
            join(this.root, 'logs'),
          ],
          environment: process.env,
          readyLine: 'hub listening on ',
          label: 'hub',
        });
      } catch (error) {
        lastError = error;
        await delay(RESTART_DELAY_MS);
      }
    }
    throw new Error(`hub 重启失败: ${String(lastError)}`);
  }

  private async registerNode(): Promise<void> {
    await this.request('POST', '/api/nodes', { name: this.nodeName, url: this.agent.url });
    await this.waitForNode();
  }

  private async waitForNode(): Promise<void> {
    for (let attempt = 0; attempt < READY_POLL_ATTEMPTS; attempt += 1) {
      if (await this.nodeReady()) {
        return;
      }
      await delay(READY_POLL_INTERVAL_MS);
    }
    throw new Error(`节点 ${this.nodeName} 未就绪`);
  }

  /// hub 重启期间端口尚未监听，fetch 以连接错误拒绝；此期间按未就绪处理。
  private async nodeReady(): Promise<boolean> {
    try {
      const bootstrap = await this.bootstrap();
      const node = bootstrap.nodes.find((entry) => entry.name === this.nodeName);
      return node !== undefined && node.status === 'ok' && node.bootstrap !== null;
    } catch {
      return false;
    }
  }

  private async request<T>(method: string, path: string, body?: unknown): Promise<T> {
    const init: RequestInit = { method };
    if (body !== undefined) {
      init.headers = { 'content-type': 'application/json' };
      init.body = JSON.stringify(body);
    }
    const response = await fetch(`${this.hubUrl}${path}`, init);
    if (!response.ok) {
      throw new Error(`${method} ${path} 失败: ${response.status} ${await response.text()}`);
    }
    if (response.status === 204) {
      return undefined as T;
    }
    return (await response.json()) as T;
  }
}

async function binaryPath(name: string): Promise<string> {
  const path = join(repositoryRoot, 'target', 'debug', name);
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
