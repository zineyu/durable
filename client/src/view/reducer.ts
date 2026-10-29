import type { BootstrapResponse } from '../api/generated/BootstrapResponse';
import type { MessageBody } from '../api/generated/MessageBody';
import type { SessionMetadata } from '../api/generated/SessionMetadata';
import type { SessionTreeSnapshot } from '../api/generated/SessionTreeSnapshot';
import type { StreamEvent } from '../api/generated/StreamEvent';
import {
  applyDelta,
  applyMessageBodies,
  failMessageBodies,
  requestMessageBodies,
  retryMessageBodies,
} from './bodies';
import {
  applyFact,
  applyRetry,
  clearRetry,
  emptyClientState,
  loadBootstrap,
  loadSessionTree,
  markSeen,
  storeSession,
} from './state';
import type { ClientState } from './state';
import type { ClientError, ConnectionState } from './types';

/// 实时事件只含事实、增量与重试。`resync` 由事件流自身处理（断开并刷新快照）。
export type StreamFactEvent = Exclude<StreamEvent, { type: 'resync' }>;

export type ClientAction =
  | { type: 'bootstrap'; bootstrap: BootstrapResponse }
  | { type: 'stream'; event: StreamFactEvent }
  | { type: 'tree'; tree: SessionTreeSnapshot }
  | { type: 'session'; session: SessionMetadata }
  | { type: 'bodies-request'; ids: number[] }
  | { type: 'bodies-failed'; ids: number[] }
  | { type: 'bodies-retry'; ids: number[] }
  | { type: 'bodies'; ids: number[]; messages: MessageBody[] }
  | { type: 'seen'; sessionId: string; at: number }
  | { type: 'connection'; connection: ConnectionState }
  | { type: 'error'; error: ClientError }
  | { type: 'dismiss-error' };

export { emptyClientState as initialClientState };
export type { ClientState };

export function reduce(state: ClientState, action: ClientAction): ClientState {
  switch (action.type) {
    case 'bootstrap':
      return loadBootstrap(state, action.bootstrap);
    case 'stream':
      return applyStreamEvent(state, action.event);
    case 'tree':
      return loadSessionTree(state, action.tree);
    case 'session':
      return storeSession(state, action.session);
    case 'bodies-request':
      return requestMessageBodies(state, action.ids);
    case 'bodies-failed':
      return failMessageBodies(state, action.ids);
    case 'bodies-retry':
      return retryMessageBodies(state, action.ids);
    case 'bodies':
      return applyMessageBodies(state, action.ids, action.messages);
    case 'seen':
      return markSeen(state, action.sessionId, action.at);
    case 'connection':
      return { ...state, connection: action.connection };
    case 'error':
      return { ...state, error: action.error };
    case 'dismiss-error':
      return { ...state, error: null };
    default: {
      const unhandled: never = action;
      throw new Error(`未处理的动作: ${JSON.stringify(unhandled)}`);
    }
  }
}

export function applyStreamEvent(state: ClientState, event: StreamFactEvent): ClientState {
  switch (event.type) {
    case 'fact': {
      if (state.lastEventId !== null && event.id <= state.lastEventId) {
        return state;
      }
      return applyFact(state, { id: event.id, session: event.session, event: event.event });
    }
    case 'delta':
      return clearRetry(
        applyDelta(state, event.sessionId, event.messageId, event.kind, event.text),
        event.sessionId,
      );
    case 'retry':
      if (!state.sessions.some((session) => session.id === event.sessionId)) {
        return state;
      }
      return applyRetry(state, event.sessionId, {
        attempt: event.attempt,
        maxAttempts: event.maxAttempts,
        delayMs: event.delayMs,
      });
    default: {
      const unhandled: never = event;
      throw new Error(`未处理的事件: ${JSON.stringify(unhandled)}`);
    }
  }
}
