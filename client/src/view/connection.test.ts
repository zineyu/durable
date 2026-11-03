import { describe, expect, it } from 'vitest';

import {
  DISCONNECT_AFTER_FAILURES,
  connectionFailed,
  connectionNeverOpened,
  connectionOpened,
  freshReconnectProgress,
} from './connection';
import type { ReconnectProgress } from './connection';
import type { ConnectionState } from './types';

/// 连续失败直到状态为「已断开」，返回每次尝试后的状态。
function failUntilDisconnected(start: ReconnectProgress): ConnectionState[] {
  const states: ConnectionState[] = [];
  let progress = start;
  for (let attempt = 0; attempt < DISCONNECT_AFTER_FAILURES; attempt += 1) {
    const result = connectionFailed(progress);
    progress = result.progress;
    states.push(result.state);
  }
  return states;
}

describe('连接状态', () => {
  it('已连上后重连失败先停在重连中，连续达到阈值后升级为已断开', () => {
    const opened = connectionOpened();
    const states = failUntilDisconnected(opened.progress);

    expect(states.slice(0, -1).every((state) => state === 'reconnecting')).toBe(true);
    expect(states.at(-1)).toBe('disconnected');
  });

  it('一次成功打开即回到已连接，并重新从零计数', () => {
    const failures = failUntilDisconnected(connectionOpened().progress);
    expect(failures.at(-1)).toBe('disconnected');

    const opened = connectionOpened();
    expect(opened.state).toBe('connected');
    expect(opened.progress).toEqual({ failures: 0, everConnected: true });
    expect(connectionFailed(opened.progress).state).toBe('reconnecting');
  });

  it('没有连上过时任何失败都保持已断开', () => {
    const never = connectionNeverOpened();
    expect(never.progress).toEqual(freshReconnectProgress);
    expect(never.state).toBe('disconnected');
    expect(failUntilDisconnected(never.progress).every((state) => state === 'disconnected'))
      .toBe(true);
  });
});
