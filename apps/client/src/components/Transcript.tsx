import type { ReactElement, ReactNode } from 'react';
import { useTranscriptScroll } from '../view/use-transcript-scroll';
import type { SessionId } from '../view/ids';
import styles from './Transcript.module.css';

export type TranscriptProps = {
  sessionId: SessionId;
  children: ReactNode;
};

/// 对话记录。滚动位置由 `view/use-transcript-scroll.ts` 按 session 保存与恢复。
export function Transcript({ sessionId, children }: TranscriptProps): ReactElement {
  const ref = useTranscriptScroll(sessionId);
  return (
    <div ref={ref} className={styles.transcript} role="log" aria-label="对话记录">
      <div className={styles.inner}>{children}</div>
    </div>
  );
}
