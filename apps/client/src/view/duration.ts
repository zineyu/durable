import type { MessageSkeleton } from '../api/generated/MessageSkeleton';
import type { RunRecord } from '../api/generated/RunRecord';
import type { RunDuration } from './types';

/// 本模块负责耗时文本与 run 耗时的挂载位置：不足一秒用整数毫秒，其余用一位小数的秒。
/// 界面与导出共用这里的文本。不属于它的：测量在 agent 的调用点，持久化在 run 与工具结果。

export function formatDuration(milliseconds: number): string {
  if (milliseconds < 1000) {
    return `${String(milliseconds)}ms`;
  }
  return `${(milliseconds / 1000).toFixed(1).replace(/\.0$/, '')}s`;
}

/// run 耗时的同一句话，消息尾部与 Markdown 导出共用。
export function runDurationSentence(duration: RunDuration): string {
  return `本次 run 耗时 ${duration.label}`;
}

/// 把每条已结束且有耗时的 run 挂到它收尾的 assistant 消息上。
///
/// 收尾消息是同 `runId` 的 assistant 消息里链上最后一条。没有收尾消息的 run 不出现；
/// 进行中的 run 不做实时计时。
export function closingRunDurations(
  nodes: MessageSkeleton[],
  runs: RunRecord[],
): Map<number, RunDuration> {
  const closing = new Map<number, number>();
  for (const node of nodes) {
    if (node.role === 'assistant' && node.runId !== null) {
      closing.set(node.runId, node.id);
    }
  }
  const durations = new Map<number, RunDuration>();
  for (const run of runs) {
    if (run.status === 'running' || run.durationMs === null) {
      continue;
    }
    const messageId = closing.get(run.id);
    if (messageId !== undefined) {
      durations.set(messageId, {
        label: formatDuration(run.durationMs),
        endedAt: run.updatedAt,
      });
    }
  }
  return durations;
}
