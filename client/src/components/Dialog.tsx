import type { ReactElement, ReactNode } from 'react';
import { Button } from './Button';
import styles from './Dialog.module.css';

export type DialogProps = {
  title: string;
  confirmLabel: string;
  onConfirm: () => void;
  onCancel: () => void;
  children?: ReactNode;
  confirmDisabled?: boolean;
  danger?: boolean;
};

export function Dialog({
  title,
  confirmLabel,
  onConfirm,
  onCancel,
  children,
  confirmDisabled = false,
  danger = false,
}: DialogProps): ReactElement {
  return (
    <div className={styles.overlay} onClick={onCancel}>
      <div
        className={styles.panel}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        onClick={(event) => { event.stopPropagation(); }}
      >
        <h2 className={styles.title}>{title}</h2>
        {children !== undefined ? <div className={styles.body}>{children}</div> : null}
        <div className={styles.actions}>
          <Button variant="secondary" onClick={onCancel}>
            取消
          </Button>
          <Button
            variant={danger ? 'danger' : 'primary'}
            onClick={onConfirm}
            disabled={confirmDisabled}
          >
            {confirmLabel}
          </Button>
        </div>
      </div>
    </div>
  );
}
