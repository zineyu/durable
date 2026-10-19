import type { ConnectionState } from '../view/types';
import type { StreamEvent } from './generated/StreamEvent';

export type EventStreamHandlers = {
  onEvent: (event: StreamEvent) => void;
  onState: (state: ConnectionState) => void;
};

export type EventStream = {
  close: () => void;
};

const RECONNECT_DELAY_MS = 1000;

/// 订阅全局事件流；断线后用最后应用的事件 id 续传。
export function openEventStream(after: number, handlers: EventStreamHandlers): EventStream {
  let cursor = after;
  let source: EventSource | null = null;
  let timer: ReturnType<typeof setTimeout> | null = null;
  let closed = false;

  function connect() {
    if (closed) {
      return;
    }
    source = new EventSource(`/api/events?after=${cursor}`);
    source.onopen = () => handlers.onState('connected');
    source.onmessage = (message) => {
      const data: unknown = JSON.parse(message.data);
      const event = data as StreamEvent;
      if (event.type === 'fact' && event.id > cursor) {
        cursor = event.id;
      }
      handlers.onEvent(event);
    };
    source.onerror = () => {
      handlers.onState('reconnecting');
      source?.close();
      source = null;
      schedule();
    };
  }

  function schedule() {
    if (closed) {
      return;
    }
    timer = setTimeout(connect, RECONNECT_DELAY_MS);
  }

  connect();

  return {
    close: () => {
      closed = true;
      if (timer !== null) {
        clearTimeout(timer);
      }
      source?.close();
    },
  };
}
