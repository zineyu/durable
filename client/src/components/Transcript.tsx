import type { ReactElement, ReactNode } from 'react';
import styles from './Transcript.module.css';

export type TranscriptProps = {
  children: ReactNode;
};

export function Transcript({ children }: TranscriptProps): ReactElement {
  return (
    <div className={styles.transcript}>
      <div className={styles.inner}>{children}</div>
    </div>
  );
}
