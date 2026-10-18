import type { ReactNode } from 'react';
import styles from './Transcript.module.css';

export type TranscriptProps = {
  children: ReactNode;
};

export function Transcript({ children }: TranscriptProps) {
  return (
    <div className={styles.transcript}>
      <div className={styles.inner}>{children}</div>
    </div>
  );
}
