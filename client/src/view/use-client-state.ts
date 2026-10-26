import { useCallback, useEffect, useReducer, useRef } from 'react';
import type { Dispatch } from 'react';
import { api } from '../api/client';
import type { ApiFailure, ApiResult } from '../api/client';
import { openEventStream } from '../api/events';
import type { EventStream } from '../api/events';
import type { HubBootstrap } from '../api/generated/HubBootstrap';
import { hasSession, missingShards } from './derived';
import { snapshotCursor } from './node-state';
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

/// 客户端状态与网络接线：首屏 bootstrap、合并事件流、断线重连与正文拉取。
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
    cursorRef.current = state.hub.cursor;
  }, [state.hub.cursor]);

  useEffect(() => {
    selectionRef.current = selection;
  }, [selection]);

  const missingKey = missingShards(state.hub.nodes, state.nodes).join(',');

  useEffect(() => {
    if (missingKey === '') {
      return;
    }
    const liveness = new AbortController();
    void (async () => {
      for (const name of missingKey.split(',')) {
        const outcome = await api.nodeBootstrap(name);
        if (liveness.signal.aborted) {
          return;
        }
        if (!outcome.ok) {
          report(outcome.error);
          continue;
        }
        dispatch({ type: 'node-bootstrap', node: name, bootstrap: outcome.value });
      }
    })();
    return () => {
      liveness.abort();
    };
  }, [missingKey, report]);

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
      const cursor = snapshotCursor(cursorRef.current, outcome.value.maxHubSeq);
      dispatch({ type: 'bootstrap', bootstrap: outcome.value });
      const current = selectionRef.current;
      if (current.node !== null && current.sessionId !== null) {
        const tree = await api.sessionTree(current.node, current.sessionId);
        if (liveness.signal.aborted) {
          return null;
        }
        if (!tree.ok) {
          report(tree.error);
          return null;
        }
        dispatch({ type: 'tree', node: current.node, tree: tree.value });
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

    const connect = (bootstrap: HubBootstrap) => {
      const cursor = snapshotCursor(cursorRef.current, bootstrap.maxHubSeq);
      dispatch({ type: 'bootstrap', bootstrap });
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
      connect(outcome.value);
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

  const shard = selection.node === null ? undefined : state.nodes[selection.node];
  const sessionExists =
    shard !== undefined &&
    selection.sessionId !== null &&
    hasSession(shard, selection.sessionId);

  useEffect(() => {
    if (selection.node === null || selection.sessionId === null || !sessionExists) {
      return;
    }
    const node = selection.node;
    const id = selection.sessionId;
    const liveness = new AbortController();
    void (async () => {
      const outcome = await api.sessionTree(node, id);
      if (liveness.signal.aborted) {
        return;
      }
      if (!outcome.ok) {
        report(outcome.error);
        return;
      }
      dispatch({ type: 'tree', node, tree: outcome.value });
    })();
    return () => {
      liveness.abort();
    };
  }, [selection.node, selection.sessionId, sessionExists, report]);

  useMessageBodies({
    node: selection.node,
    sessionId: selection.sessionId,
    shard,
    dispatch,
    onFailure: report,
  });

  return { state, dispatch, report, runCommand };
}
