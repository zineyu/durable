import type { ConnectionState } from './types';

/// 连续重连失败达到该次数即升级为「已断开」。重连等待是 1 秒，因此约 3 秒后可见。
/// 规格 user story R-49：连接状态显式可见。
export const DISCONNECT_AFTER_FAILURES = 3;

/// 重连进度：连续失败次数，以及本次页面运行里是否成功连上过。
/// 没有连上过时任何失败都保持「已断开」，不必等计数达到阈值。
export type ReconnectProgress = {
  readonly failures: number;
  readonly everConnected: boolean;
};

/// 一次连接尝试的结局与它对应的连接状态。
export type ConnectionAttempt = {
  readonly progress: ReconnectProgress;
  readonly state: ConnectionState;
};

/// 尚未连上过的初始进度。
export const freshReconnectProgress: ReconnectProgress = {
  failures: 0,
  everConnected: false,
};

/// 连接已经建立：连接状态回到「已连接」，失败计数归零。
export function connectionOpened(): ConnectionAttempt {
  return {
    progress: { failures: 0, everConnected: true },
    state: 'connected',
  };
}

/// 首次 bootstrap 失败：连接状态直接是「已断开」。
export function connectionNeverOpened(): ConnectionAttempt {
  return { progress: freshReconnectProgress, state: 'disconnected' };
}

/// 一次重连失败：连续失败达到阈值时升级为「已断开」，否则保持「重连中」。
export function connectionFailed(progress: ReconnectProgress): ConnectionAttempt {
  const next: ReconnectProgress = {
    failures: progress.failures + 1,
    everConnected: progress.everConnected,
  };
  return { progress: next, state: reconnectState(next) };
}

/// 当前进度对应的连接状态。
export function reconnectState(progress: ReconnectProgress): ConnectionState {
  if (!progress.everConnected) {
    return 'disconnected';
  }
  return progress.failures >= DISCONNECT_AFTER_FAILURES ? 'disconnected' : 'reconnecting';
}
