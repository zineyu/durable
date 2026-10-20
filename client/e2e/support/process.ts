import { spawn } from 'node:child_process';
import type { ChildProcess } from 'node:child_process';

/// 已就绪的二进制进程。监听地址取自它的启动输出。
export type LaunchedProcess = {
  readonly url: string;
  stop(): Promise<void>;
};

export type LaunchOptions = {
  binary: string;
  args: string[];
  environment: NodeJS.ProcessEnv;
  /// 启动输出中携带监听地址的行前缀，例如 `agent listening on `。
  readyLine: string;
  label: string;
};

const READY_TIMEOUT_MS = 20_000;
const STOP_TIMEOUT_MS = 5_000;

/// 启动二进制并等待它打印监听地址。
///
/// 失败：二进制不存在、未在超时内就绪、或提前退出。
export async function launchProcess(options: LaunchOptions): Promise<LaunchedProcess> {
  const child = spawn(options.binary, options.args, {
    env: options.environment,
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  const stdout = child.stdout;
  const stderr = child.stderr;
  if (stdout === null || stderr === null) {
    throw new Error(`${options.label} 未提供标准输出`);
  }

  const errors: string[] = [];
  const url = await readReadyUrl(options, child, stdout, stderr, errors);
  return { url, stop: () => stop(child, options.label) };
}

function readReadyUrl(
  options: LaunchOptions,
  child: ChildProcess,
  stdout: NodeJS.ReadableStream,
  stderr: NodeJS.ReadableStream,
  errors: string[],
): Promise<string> {
  return new Promise<string>((resolve, reject) => {
    const timer = setTimeout(() => {
      reject(
        new Error(
          `${options.label} 未在 ${READY_TIMEOUT_MS / 1000} 秒内就绪。stderr:\n${errors.join('')}`,
        ),
      );
    }, READY_TIMEOUT_MS);

    stdout.setEncoding('utf8');
    stdout.on('data', (chunk: string) => {
      for (const line of chunk.split('\n')) {
        if (!line.startsWith(options.readyLine)) {
          continue;
        }
        clearTimeout(timer);
        resolve(line.slice(options.readyLine.length).trim());
      }
    });

    stderr.setEncoding('utf8');
    stderr.on('data', (chunk: string) => {
      errors.push(chunk);
    });

    child.once('error', (error) => {
      clearTimeout(timer);
      reject(new Error(`无法启动 ${options.label}: ${error.message}`));
    });

    child.once('exit', (code, signal) => {
      clearTimeout(timer);
      reject(
        new Error(
          `${options.label} 提前退出: code=${String(code)} signal=${String(signal)}\n${errors.join('')}`,
        ),
      );
    });
  });
}

async function stop(child: ChildProcess, label: string): Promise<void> {
  if (child.exitCode !== null || child.signalCode !== null) {
    return;
  }
  const exited = new Promise<void>((resolve) => {
    child.once('exit', () => resolve());
  });
  child.kill('SIGKILL');
  const timeout = new Promise<never>((_, reject) => {
    setTimeout(
      () => reject(new Error(`${label} 未在 ${STOP_TIMEOUT_MS / 1000} 秒内退出`)),
      STOP_TIMEOUT_MS,
    ).unref();
  });
  await Promise.race([exited, timeout]);
}
