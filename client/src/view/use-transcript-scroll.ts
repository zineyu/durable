import { useEffect, useLayoutEffect, useRef } from 'react';
import type { RefObject } from 'react';
import { readScroll, writeScroll } from './scroll';
import type { KeyValueStorage } from './storage';
import type { SessionId } from './ids';

/// 一次恢复的进度：希望到达的位置、上一次实际位置与是否已经安定。
type ScrollTrack = {
  target: number;
  expected: number;
  restored: boolean;
};

/// 小于 1 像素的差异视同相同，避免浏览器的亚像素取整把程序滚动当成用户滚动。
const SAME_OFFSET_PX = 1;

/// transcript 的滚动位置按 session 记在浏览器存储里，刷新或切回该 session 时恢复。
/// 消息正文异步到达，内容高度会增长，因此每次渲染后都重试一次，直到位置达成或用户自己滚动。
/// 规格 user story R-45：刷新后恢复选中 session 与滚动位置。
export function useTranscriptScroll(
  sessionId: SessionId,
  storage: KeyValueStorage = window.localStorage,
): RefObject<HTMLDivElement | null> {
  const elementRef = useRef<HTMLDivElement | null>(null);
  const track = useRef<ScrollTrack>({ target: 0, expected: 0, restored: true });

  useLayoutEffect(() => {
    const offset = readScroll(storage, sessionId);
    track.current = { target: offset, expected: offset, restored: offset === 0 };
  }, [sessionId, storage]);

  useLayoutEffect(() => {
    const element = elementRef.current;
    if (element === null || track.current.restored) {
      return;
    }
    element.scrollTop = track.current.target;
    track.current.expected = element.scrollTop;
    track.current.restored = Math.abs(element.scrollTop - track.current.target) < SAME_OFFSET_PX;
  });

  useEffect(() => {
    const element = elementRef.current;
    if (element === null) {
      return;
    }
    const onScroll = () => {
      const offset = element.scrollTop;
      if (Math.abs(offset - track.current.expected) < SAME_OFFSET_PX) {
        return;
      }
      track.current = { target: offset, expected: offset, restored: true };
      writeScroll(storage, sessionId, offset);
    };
    element.addEventListener('scroll', onScroll, { passive: true });
    return () => {
      element.removeEventListener('scroll', onScroll);
    };
  }, [sessionId, storage]);

  return elementRef;
}
