import { createServer } from 'node:http';
import type { IncomingMessage, Server, ServerResponse } from 'node:http';

/// 一条 assistant 消息里的单个工具调用。
export type ToolCallScript = {
  id: string;
  name: string;
  arguments: string;
};

/// 一次模型请求的脚本。脚本按请求顺序消费。
export type ModelScript =
  | { kind: 'text'; text: string }
  | { kind: 'tool-calls'; calls: ToolCallScript[] }
  /// 先流式返回文本，再保持连接不结束，直到调用 `release()`。
  | { kind: 'hold'; text: string }
  | { kind: 'fail'; status: number };

const ENDPOINT = '/v1/chat/completions';
const CLOSE_TIMEOUT_MS = 5_000;
const HOLD_TIMEOUT_MS = 30_000;

/// 脚本化的 OpenAI 兼容 SSE 服务。它是真实 HTTP 服务，不替换被测代码。
export class ModelServer {
  readonly baseUrl: string;
  private readonly server: Server;
  private readonly scripts: ModelScript[] = [];
  private readonly bodies: unknown[] = [];
  private readonly held = new Map<ServerResponse, NodeJS.Timeout>();

  private constructor(server: Server, baseUrl: string) {
    this.server = server;
    this.baseUrl = baseUrl;
    server.on('request', this.handle);
  }

  static async start(): Promise<ModelServer> {
    const server = createServer();
    await new Promise<void>((resolve, reject) => {
      server.once('error', reject);
      server.listen(0, '127.0.0.1', resolve);
    });
    const address = server.address();
    if (address === null || typeof address === 'string') {
      throw new Error('脚本化模型服务未获得端口');
    }
    return new ModelServer(server, `http://127.0.0.1:${address.port}/v1`);
  }

  /// 追加脚本，供后续模型请求依次使用。
  push(...scripts: ModelScript[]): void {
    this.scripts.push(...scripts);
  }

  /// 结束所有挂起的响应，让运行中的 run 正常收尾。
  release(): void {
    for (const [response, timer] of this.held) {
      clearTimeout(timer);
      if (!response.writableEnded && !response.destroyed) {
        finishStream(response);
      }
    }
    this.held.clear();
  }

  /// 已收到的请求体，按到达顺序。
  requests(): unknown[] {
    return [...this.bodies];
  }

  async stop(): Promise<void> {
    for (const [response, timer] of this.held) {
      clearTimeout(timer);
      response.destroy();
    }
    this.held.clear();
    this.server.closeAllConnections();
    await new Promise<void>((resolve) => {
      this.server.close(() => resolve());
      setTimeout(resolve, CLOSE_TIMEOUT_MS).unref();
    });
  }

  /// 保持连接不结束。用例未显式调用 `release()` 时，兜底定时器在用例超时前收尾。
  private hold(response: ServerResponse): void {
    const timer = setTimeout(() => {
      this.held.delete(response);
      if (!response.writableEnded && !response.destroyed) {
        finishStream(response);
      }
    }, HOLD_TIMEOUT_MS);
    timer.unref();
    this.held.set(response, timer);
  }

  private readonly handle = (request: IncomingMessage, response: ServerResponse): void => {
    const chunks: Buffer[] = [];
    request.on('data', (chunk: Buffer) => {
      chunks.push(chunk);
    });
    request.on('end', () => {
      if (request.url !== ENDPOINT) {
        writeJson(response, 404, { error: { message: `未知路径: ${request.url ?? ''}` } });
        return;
      }
      this.bodies.push(JSON.parse(Buffer.concat(chunks).toString('utf8')) as unknown);
      const script = this.scripts.shift();
      if (script === undefined) {
        writeJson(response, 500, { error: { message: '没有可用脚本' } });
        return;
      }
      this.play(script, response);
    });
  };

  private play(script: ModelScript, response: ServerResponse): void {
    switch (script.kind) {
      case 'fail':
        writeJson(response, script.status, { error: { message: '上游错误' } });
        return;
      case 'text':
        startStream(response);
        writeText(response, script.text);
        finishStream(response);
        return;
      case 'tool-calls':
        startStream(response);
        writeToolCalls(response, script.calls);
        finishStream(response);
        return;
      case 'hold':
        startStream(response);
        writeText(response, script.text);
        this.hold(response);
        return;
      default: {
        const unhandled: never = script;
        throw new Error(`未处理的脚本: ${JSON.stringify(unhandled)}`);
      }
    }
  }
}

function startStream(response: ServerResponse): void {
  response.writeHead(200, {
    'content-type': 'text/event-stream',
    'cache-control': 'no-store',
    connection: 'keep-alive',
  });
}

function finishStream(response: ServerResponse): void {
  response.write('data: [DONE]\n\n');
  response.end();
}

function writeEvent(response: ServerResponse, payload: unknown): void {
  response.write(`data: ${JSON.stringify(payload)}\n\n`);
}

function writeText(response: ServerResponse, text: string): void {
  const characters = [...text];
  for (let index = 0; index < characters.length; index += 4) {
    const piece = characters.slice(index, index + 4).join('');
    writeEvent(response, { choices: [{ delta: { content: piece } }] });
  }
}

function writeToolCalls(response: ServerResponse, calls: ToolCallScript[]): void {
  writeEvent(response, {
    choices: [
      {
        delta: {
          tool_calls: calls.map((call, index) => ({
            index,
            id: call.id,
            type: 'function',
            function: { name: call.name, arguments: '' },
          })),
        },
      },
    ],
  });
  writeEvent(response, {
    choices: [
      {
        delta: {
          tool_calls: calls.map((call, index) => ({
            index,
            function: { arguments: call.arguments },
          })),
        },
      },
    ],
  });
}

function writeJson(response: ServerResponse, status: number, payload: unknown): void {
  response.writeHead(status, { 'content-type': 'application/json' });
  response.end(JSON.stringify(payload));
}
