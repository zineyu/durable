import { useCallback, useEffect, useRef } from 'react';
import type { Dispatch } from 'react';
import { api } from '../api/client';
import type { ApiFailure } from '../api/client';
import type { NodeName } from '../api/generated/NodeName';
import { missingBodyIds } from './bodies';
import type { NodeState } from './node-state';
import type { ClientAction } from './reducer';

/// 失败后的重试等待；用尽后该批正文停在骨架，等下一次快照重载。
const RETRY_DELAYS_MS = [1000, 3000];

type MessageBodiesOptions = {
  node: NodeName | null;
  sessionId: string | null;
  shard: NodeState | undefined;
  dispatch: Dispatch<ClientAction>;
  onFailure: (failure: ApiFailure) => void;
};

/// 拉取当前 transcript 缺失的正文。存活范围跟随节点与 session，不跟随待拉取的 id 集合。
export function useMessageBodies({
  node,
  sessionId,
  shard,
  dispatch,
  onFailure,
}: MessageBodiesOptions): void {
  const abortRef = useRef<AbortController | null>(null);
  const failuresRef = useRef(0);
  const timersRef = useRef<ReturnType<typeof setTimeout>[]>([]);
  const needed =
    node === null || sessionId === null || shard === undefined
      ? ''
      : missingBodyIds(shard, sessionId).join(',');

  const clearTimers = useCallback(() => {
    for (const timer of timersRef.current) {
      clearTimeout(timer);
    }
    timersRef.current = [];
  }, []);

  useEffect(() => {
    failuresRef.current = 0;
    const controller = new AbortController();
    abortRef.current = controller;
    return () => {
      controller.abort();
      clearTimers();
    };
  }, [node, sessionId, clearTimers]);

  const scheduleRetry = useCallback(
    (target: NodeName, ids: number[]) => {
      const delay = RETRY_DELAYS_MS[failuresRef.current - 1];
      if (delay === undefined) {
        return;
      }
      const timer = setTimeout(() => {
        dispatch({ type: 'bodies-retry', node: target, ids });
      }, delay);
      timersRef.current.push(timer);
    },
    [dispatch],
  );

  useEffect(() => {
    if (needed === '' || node === null || sessionId === null) {
      return;
    }
    const ids = needed.split(',').map(Number);
    const signal = abortRef.current?.signal;
    dispatch({ type: 'bodies-request', node, ids });
    void (async () => {
      const outcome = await api.messageBodies(node, sessionId, ids, signal);
      if (signal?.aborted === true) {
        return;
      }
      if (outcome.ok) {
        failuresRef.current = 0;
        dispatch({ type: 'bodies', node, ids, messages: outcome.value.messages });
        return;
      }
      failuresRef.current += 1;
      dispatch({ type: 'bodies-failed', node, ids });
      onFailure(outcome.error);
      scheduleRetry(node, ids);
    })();
  }, [needed, node, sessionId, dispatch, onFailure, scheduleRetry]);
}
