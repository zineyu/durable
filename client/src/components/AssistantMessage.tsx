import { BranchSwitcher } from './BranchSwitcher';
import styles from './AssistantMessage.module.css';
import { messageStateLabel } from '../view/labels';
import type { ReactNode } from 'react';
import type { MessageState } from '../view/types';

export type BranchNavigation = {
  index: number;
  total: number;
  onPrev?: () => void;
  onNext?: () => void;
};

export type AssistantMessageProps = {
  state: MessageState;
  children: ReactNode;
  note?: string;
  branch?: BranchNavigation;
};

export function AssistantMessage({ state, children, note, branch }: AssistantMessageProps) {
  const showStatus = state !== 'complete' || note !== undefined;
  return (
    <article className={styles.message} data-state={state}>
      <div className={styles.body}>
        {children}
        {state === 'streaming' ? <span className={styles.caret} aria-hidden="true" /> : null}
      </div>
      {showStatus ? (
        <p className={styles.status}>
          {state !== 'complete' ? (
            <span className={styles.state} data-state={state}>
              {messageStateLabel[state]}
            </span>
          ) : null}
          {note !== undefined ? <span className={styles.note}>{note}</span> : null}
        </p>
      ) : null}
      {branch !== undefined ? (
        <footer className={styles.footer}>
          <BranchSwitcher
            index={branch.index}
            total={branch.total}
            {...(branch.onPrev ? { onPrev: branch.onPrev } : {})}
            {...(branch.onNext ? { onNext: branch.onNext } : {})}
          />
        </footer>
      ) : null}
    </article>
  );
}
