import { useCallback, useEffect, useReducer, useRef } from 'react';
import type { Dispatch } from 'react';
import { api } from '../api/client';
import type { ApiFailure, ApiResult } from '../api/client';
import { openEventStream } from '../api/events';
import type { EventStream } from '../api/events';
import { hasSession } from './derived';
import { snapshotCursor } from './state';
import { initialClientState, reduce } from './reducer';
import type { ClientAction, ClientState } from './reducer';
import type { Selection } from './session-hash';
import { useMessageBodies } from './use-message-bodies';

const RECONNECT_DELAY_MS = 1000;

export type ClientController = {
  state: ClientState;
  dispatch: Dispatch<ClientAction>;
  report: (failure: ApiFailure) => void;
  runCommand: (result: Promise<ApiResult<unknown>>) => void;
};

/// 客户端状态与网络接线：一次 bootstrap、一条事件流与一个重连循环。
/// 规格 user story R-43、R-49：断线续传只有一个语义，连接状态显式可见。
/// 重新连接时重取 bootstrap 与当前 session 的树，再从已应用的事件 id 续传。
export function useClientState(selection: Selection): ClientController {
  const [state, dispatch] = useReducer(reduce, initialClientState);

  const report = useCallback((failure: ApiFailure) => {
    dispatch({ type: 'error', error: { code: failure.code, message: failure.message } });
  }, []);

  const runCommand = useCallback(
    (result: Promise<ApiResult<unknown>>) => {
      void (async () => {
        const outcome = await result;
        if (!outcome.ok) {
          report(outcome.error);
        }
      })();
    },
    [report],
  );

  const streamRef = useRef<EventStream | null>(null);
  const cursorRef = useRef<number | null>(null);
  const selectionRef = useRef<Selection>(selection);

  useEffect(() => {
    cursorRef.current = state.lastEventId;
  }, [state.lastEventId]);

  useEffect(() => {
    selectionRef.current = selection;
  }, [selection]);

  useEffect(() => {
    const liveness = new AbortController();
    let retry: ReturnType<typeof setTimeout> | null = null;

    const delay = (milliseconds: number) =>
      new Promise<void>((resolve) => {
        retry = setTimeout(resolve, milliseconds);
      });

    const refresh = async (): Promise<number | null> => {
      const outcome = await api.bootstrap();
      if (liveness.signal.aborted) {
        return null;
      }
      if (!outcome.ok) {
        report(outcome.error);
        return null;
      }
      const cursor = snapshotCursor(cursorRef.current, outcome.value.maxEventId);
      dispatch({ type: 'bootstrap', bootstrap: outcome.value });
      const current = selectionRef.current;
      if (current !== null) {
        const tree = await api.sessionTree(current);
        if (liveness.signal.aborted) {
          return null;
        }
        if (!tree.ok) {
          report(tree.error);
        } else {
          dispatch({ type: 'tree', tree: tree.value });
        }
      }
      return cursor;
    };

    const reconnect = async () => {
      for (;;) {
        const cursor = await refresh();
        if (liveness.signal.aborted) {
          return;
        }
        if (cursor !== null) {
          streamRef.current?.resume(cursor);
          return;
        }
        await delay(RECONNECT_DELAY_MS);
        if (liveness.signal.aborted) {
          return;
        }
      }
    };

    const connect = (maxEventId: number) => {
      const cursor = snapshotCursor(cursorRef.current, maxEventId);
      streamRef.current = openEventStream(cursor, {
        onEvent: (event) => { dispatch({ type: 'stream', event }); },
        onState: (connection) => { dispatch({ type: 'connection', connection }); },
        onDisconnect: () => {
          void reconnect();
        },
      });
    };

    void (async () => {
      const outcome = await api.bootstrap();
      if (liveness.signal.aborted) {
        return;
      }
      if (!outcome.ok) {
        report(outcome.error);
        dispatch({ type: 'connection', connection: 'disconnected' });
        return;
      }
      const cursor = snapshotCursor(cursorRef.current, outcome.value.maxEventId);
      dispatch({ type: 'bootstrap', bootstrap: outcome.value });
      connect(cursor);
    })();

    return () => {
      liveness.abort();
      if (retry !== null) {
        clearTimeout(retry);
      }
      streamRef.current?.close();
      streamRef.current = null;
    };
  }, [report]);

  const sessionExists = selection !== null && hasSession(state, selection);

  useEffect(() => {
    if (selection === null || !sessionExists) {
      return;
    }
    const id = selection;
    const liveness = new AbortController();
    void (async () => {
      const outcome = await api.sessionTree(id);
      if (liveness.signal.aborted) {
        return;
      }
      if (!outcome.ok) {
        report(outcome.error);
        return;
      }
      dispatch({ type: 'tree', tree: outcome.value });
    })();
    return () => {
      liveness.abort();
    };
  }, [selection, sessionExists, report]);

  useMessageBodies({
    sessionId: selection,
    state,
    dispatch,
    onFailure: report,
  });

  return { state, dispatch, report, runCommand };
}
