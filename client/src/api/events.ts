import type { ConnectionState } from '../view/types';
import type { HubStreamEvent } from './generated/HubStreamEvent';

export type EventStreamHandlers = {
  onEvent: (event: HubStreamEvent) => void;
  onState: (state: ConnectionState) => void;
  onDisconnect: () => void;
};

export type EventStream = {
  resume: (after: number) => void;
  close: () => void;
};

/// 订阅 hub 的合并事件流。连接中断或 hub 要求 resync 时通知调用方；
/// 调用方刷新合并 bootstrap 后调用 `resume` 从给定广播序号重开。
export function openEventStream(after: number, handlers: EventStreamHandlers): EventStream {
  let source: EventSource | null = null;
  let closed = false;

  function connect(cursor: number) {
    if (closed) {
      return;
    }
    const stream = new EventSource(`/api/events?after=${cursor}`);
    source = stream;
    stream.onopen = () => handlers.onState('connected');
    stream.onmessage = (message) => {
      const data: unknown = JSON.parse(message.data);
      const event = data as HubStreamEvent;
      handlers.onEvent(event);
      if (event.type === 'resync') {
        disconnect();
      }
    };
    stream.onerror = () => disconnect();
  }

  function stop() {
    const stream = source;
    source = null;
    stream?.close();
  }

  function disconnect() {
    if (source === null) {
      return;
    }
    stop();
    handlers.onState('reconnecting');
    handlers.onDisconnect();
  }

  connect(after);

  return {
    resume: (cursor) => {
      stop();
      connect(cursor);
    },
    close: () => {
      closed = true;
      stop();
    },
  };
}
