import type { ConnectionState } from '../view/types';
import type { StreamFactEvent } from '../view/reducer';
import type { StreamEvent } from './generated/StreamEvent';

export type EventStreamHandlers = {
  onEvent: (event: StreamFactEvent) => void;
  onState: (state: ConnectionState) => void;
  onDisconnect: () => void;
};

export type EventStream = {
  resume: (after: number) => void;
  close: () => void;
};

/// 订阅 agent 的单一事件流。连接中断或 agent 要求 resync 时通知调用方；
/// 调用方刷新 bootstrap 后调用 `resume` 从给定事件 id 重开。
export function openEventStream(after: number, handlers: EventStreamHandlers): EventStream {
  let source: EventSource | null = null;
  let closed = false;

  function connect(cursor: number) {
    if (closed) {
      return;
    }
    const stream = new EventSource(`/api/events?after=${cursor}`);
    source = stream;
    stream.onopen = () => { handlers.onState('connected'); };
    stream.onmessage = (message: MessageEvent<string>) => {
      const data: unknown = JSON.parse(message.data);
      const event = data as StreamEvent;
      if (event.type === 'resync') {
        disconnect();
        return;
      }
      handlers.onEvent(event);
    };
    stream.onerror = () => { disconnect(); };
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
