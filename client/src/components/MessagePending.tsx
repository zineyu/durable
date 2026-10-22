import type { ReactElement } from 'react';
import type { PartSkeleton } from '../api/generated/PartSkeleton';
import styles from './MessagePending.module.css';

export type MessagePendingProps = {
  parts: PartSkeleton[];
};

/// 骨架已到、正文未到时的占位。块数与骨架一致，不闪空白。
export function MessagePending({ parts }: MessagePendingProps): ReactElement {
  const count = parts.length === 0 ? 1 : parts.length;
  return (
    <div className={styles.pending} role="status" aria-label="正在加载消息正文">
      {Array.from({ length: count }, (_, index) => (
        <span key={index} className={styles.bar} />
      ))}
    </div>
  );
}
