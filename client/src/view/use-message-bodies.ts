import { useCallback, useEffect, useMemo, useRef } from 'react';
import type { Dispatch } from 'react';
import { api } from '../api/client';
import type { ApiFailure } from '../api/client';
import { retrying } from './body-cache';
import { missingBodyIds } from './derived';
import type { ClientState } from './state';
import type { ClientAction } from './reducer';

type MessageBodiesOptions = {
  sessionId: string | null;
  state: ClientState;
  dispatch: Dispatch<ClientAction>;
  onFailure: (failure: ApiFailure) => void;
};

/// 拉取当前 transcript 缺失的正文，并按正文缓存给出的等待档位重试失败的批次。
/// 尝试次数与等待档位都在 `body-cache`，这里只执行时间。
export function useMessageBodies({
  sessionId,
  state,
  dispatch,
  onFailure,
}: MessageBodiesOptions): void {
  const abortRef = useRef<AbortController | null>(null);
  const timersRef = useRef<ReturnType<typeof setTimeout>[]>([]);
  const scheduledRef = useRef(new Set<string>());
  const needed = sessionId === null ? '' : missingBodyIds(state, sessionId).join(',');
  const schedule = useMemo(() => retrying(state.bodies), [state.bodies]);

  const clearTimers = useCallback(() => {
    for (const timer of timersRef.current) {
      clearTimeout(timer);
    }
    timersRef.current = [];
    scheduledRef.current.clear();
  }, []);

  useEffect(() => {
    const controller = new AbortController();
    abortRef.current = controller;
    return () => {
      controller.abort();
    };
  }, [sessionId]);

  useEffect(() => {
    return () => {
      clearTimers();
    };
  }, [clearTimers]);

  useEffect(() => {
    if (needed === '' || sessionId === null) {
      return;
    }
    const ids = needed.split(',').map(Number);
    const signal = abortRef.current?.signal;
    dispatch({ type: 'bodies-request', ids });
    void (async () => {
      const outcome = await api.messageBodies(sessionId, ids, signal);
      if (signal?.aborted === true) {
        return;
      }
      if (outcome.ok) {
        dispatch({ type: 'bodies', ids, messages: outcome.value.messages });
        return;
      }
      dispatch({ type: 'bodies-failed', ids });
      onFailure(outcome.error);
    })();
  }, [needed, sessionId, dispatch, onFailure]);

  useEffect(() => {
    for (const group of schedule) {
      const key = `${group.delayMs}:${group.ids.join('.')}`;
      if (scheduledRef.current.has(key)) {
        continue;
      }
      scheduledRef.current.add(key);
      const timer = setTimeout(() => {
        scheduledRef.current.delete(key);
        dispatch({ type: 'bodies-retry', ids: group.ids });
      }, group.delayMs);
      timersRef.current.push(timer);
    }
  }, [schedule, dispatch]);
}
