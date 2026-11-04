import type { SessionId } from './ids';
import type { KeyValueStorage } from './storage';

const SCROLL_PREFIX = 'durable:scroll';

/// 小于 1 像素的偏移视同顶部：既不记录，也不恢复。
const MIN_OFFSET_PX = 1;

export function scrollKey(sessionId: SessionId): string {
  return `${SCROLL_PREFIX}:${sessionId}`;
}

/// 上次离开该 session 时 transcript 的滚动位置。没有记录或记录不可用时为 0。
export function readScroll(storage: KeyValueStorage, sessionId: SessionId): number {
  const raw = storage.getItem(scrollKey(sessionId));
  if (raw === null) {
    return 0;
  }
  const offset = Number(raw);
  return Number.isFinite(offset) && offset >= MIN_OFFSET_PX ? offset : 0;
}

/// 记录滚动位置。回到顶部时删除记录，存储里不留无意义的 0。
export function writeScroll(
  storage: KeyValueStorage,
  sessionId: SessionId,
  offset: number,
): void {
  const key = scrollKey(sessionId);
  if (!Number.isFinite(offset) || offset < MIN_OFFSET_PX) {
    storage.removeItem(key);
    return;
  }
  storage.setItem(key, String(Math.round(offset)));
}
