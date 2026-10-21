import { BranchSwitcher } from './BranchSwitcher';
import { IconButton } from './IconButton';
import styles from './AssistantMessage.module.css';
import { messageStateLabel } from '../view/labels';
import type { ReactElement, ReactNode } from 'react';
import type { BranchControl, MessageState } from '../view/types';

export type AssistantMessageProps = {
  state: MessageState;
  children: ReactNode;
  note?: string;
  branch?: BranchControl;
  onRetry?: () => void;
  retryDisabled?: boolean;
};

export function AssistantMessage({
  state,
  children,
  note,
  branch,
  onRetry,
  retryDisabled = false,
}: AssistantMessageProps): ReactElement {
  const showStatus = state !== 'complete' || note !== undefined;
  const showFooter = branch !== undefined || onRetry !== undefined;
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
      {showFooter ? (
        <footer className={styles.footer}>
          {onRetry !== undefined ? (
            <IconButton icon="retry" label="重试" onClick={onRetry} disabled={retryDisabled} />
          ) : null}
          {branch !== undefined ? (
            <BranchSwitcher
              index={branch.index}
              total={branch.total}
              {...(branch.onPrev ? { onPrev: branch.onPrev } : {})}
              {...(branch.onNext ? { onNext: branch.onNext } : {})}
            />
          ) : null}
        </footer>
      ) : null}
    </article>
  );
}
