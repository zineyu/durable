import { describe, expect, it } from 'vitest';

import type { MessageSkeleton } from '../api/generated/MessageSkeleton';
import type { RunRecord } from '../api/generated/RunRecord';
import { closingRunDurations, formatDuration, runDurationSentence } from './duration';

function message(
  id: number,
  role: MessageSkeleton['role'],
  runId: number | null,
): MessageSkeleton {
  return { id, parentId: null, role, interrupted: false, runId, createdAt: id, parts: [] };
}

function run(
  id: number,
  status: RunRecord['status'],
  durationMs: number | null,
  updatedAt: number,
): RunRecord {
  return {
    id,
    sessionId: 's-1',
    status,
    failure: null,
    progress: null,
    durationMs,
    createdAt: 1,
    updatedAt,
  };
}

describe('耗时格式', () => {
  it('不足一秒用整数毫秒', () => {
    expect(formatDuration(0)).toBe('0ms');
    expect(formatDuration(412)).toBe('412ms');
    expect(formatDuration(999)).toBe('999ms');
  });

  it('一秒及以上用一位小数的秒并去掉多余的 .0', () => {
    expect(formatDuration(1000)).toBe('1s');
    expect(formatDuration(4200)).toBe('4.2s');
    expect(formatDuration(120500)).toBe('120.5s');
  });

  it('run 耗时的同一句话供界面与导出共用', () => {
    expect(runDurationSentence({ label: '4.2s', endedAt: 1 })).toBe('本次 run 耗时 4.2s');
  });
});

describe('run 耗时挂载位置', () => {
  it('只挂在同 run 的收尾 assistant 消息上', () => {
    const nodes = [
      message(1, 'user', 1),
      message(2, 'assistant', 1),
      message(3, 'tool', 1),
      message(4, 'assistant', 1),
      message(5, 'assistant', 1),
    ];
    const durations = closingRunDurations(nodes, [run(1, 'succeeded', 4200, 99)]);

    expect([...durations]).toEqual([[5, { label: '4.2s', endedAt: 99 }]]);
  });

  it('进行中的 run 与没有耗时的 run 不显示', () => {
    const nodes = [message(1, 'assistant', 1), message(2, 'assistant', 2)];

    expect(
      closingRunDurations(nodes, [run(1, 'running', 10, 5), run(2, 'failed', null, 6)]).size,
    ).toBe(0);
  });

  it('没有收尾 assistant 消息的 run 不出现', () => {
    const nodes = [message(1, 'user', 1)];

    expect(closingRunDurations(nodes, [run(1, 'failed', 30, 8)]).size).toBe(0);
  });
});
