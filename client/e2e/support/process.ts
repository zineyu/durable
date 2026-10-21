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
/// 失败：二进制不存在、未在超时内就绪、或提前退出。失败时不留下运行中的子进程。
export async function launchProcess(options: LaunchOptions): Promise<LaunchedProcess> {
  const child = spawn(options.binary, options.args, {
    env: options.environment,
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  const stdout = child.stdout;
  const stderr = child.stderr;

  const errors: string[] = [];
  let url: string;
  try {
    url = await readReadyUrl(options, child, stdout, stderr, errors);
  } catch (error) {
    await stop(child, options.label).catch(() => undefined);
    throw error;
  }
  return { url, stop: () => stop(child, options.label) };
}

/// 累积标准输出并按行匹配监听地址。输出不保证按行切分，因此保留未完结的尾行。
function readReadyUrl(
  options: LaunchOptions,
  child: ChildProcess,
  stdout: NodeJS.ReadableStream,
  stderr: NodeJS.ReadableStream,
  errors: string[],
): Promise<string> {
  return new Promise<string>((resolve, reject) => {
    let pending = '';
    let settled = false;
    const settle = (outcome: { url: string } | { error: Error }): void => {
      if (settled) {
        return;
      }
      settled = true;
      clearTimeout(timer);
      stdout.off('data', onStdout);
      stderr.off('data', onStderr);
      child.off('error', onError);
      child.off('exit', onExit);
      stdout.resume();
      stderr.resume();
      if ('error' in outcome) {
        reject(outcome.error);
        return;
      }
      resolve(outcome.url);
    };
    const timer = setTimeout(() => {
      settle({
        error: new Error(
          `${options.label} 未在 ${READY_TIMEOUT_MS / 1000} 秒内就绪。stderr:\n${errors.join('')}`,
        ),
      });
    }, READY_TIMEOUT_MS);

    const onStdout = (chunk: string): void => {
      pending += chunk;
      const lines = pending.split('\n');
      pending = lines.pop() ?? '';
      for (const line of lines) {
        if (!line.startsWith(options.readyLine)) {
          continue;
        }
        settle({ url: line.slice(options.readyLine.length).trim() });
        return;
      }
    };
    const onStderr = (chunk: string): void => {
      errors.push(chunk);
    };
    const onError = (error: Error): void => {
      settle({ error: new Error(`无法启动 ${options.label}: ${error.message}`) });
    };
    const onExit = (code: number | null, signal: NodeJS.Signals | null): void => {
      settle({
        error: new Error(
          `${options.label} 提前退出: code=${String(code)} signal=${String(signal)}\n${errors.join('')}`,
        ),
      });
    };

    stdout.setEncoding('utf8');
    stdout.on('data', onStdout);
    stderr.setEncoding('utf8');
    stderr.on('data', onStderr);
    child.once('error', onError);
    child.once('exit', onExit);
  });
}

/// 结束子进程：先请求 `SIGTERM` 收尾，超时后升级为 `SIGKILL`。
async function stop(child: ChildProcess, label: string): Promise<void> {
  if (hasExited(child)) {
    return;
  }
  const exited = new Promise<void>((resolve) => {
    child.once('exit', () => { resolve(); });
  });
  if (await terminate(child, 'SIGTERM', exited)) {
    return;
  }
  if (await terminate(child, 'SIGKILL', exited)) {
    return;
  }
  throw new Error(`${label} 未在 ${STOP_TIMEOUT_MS / 1000} 秒内退出`);
}

function hasExited(child: ChildProcess): boolean {
  return child.exitCode !== null || child.signalCode !== null;
}

async function terminate(
  child: ChildProcess,
  signal: NodeJS.Signals,
  exited: Promise<void>,
): Promise<boolean> {
  child.kill(signal);
  return await Promise.race([exited.then(() => true), stopTimer()]);
}

function stopTimer(): Promise<false> {
  return new Promise<false>((resolve) => {
    setTimeout(() => { resolve(false); }, STOP_TIMEOUT_MS).unref();
  });
}
