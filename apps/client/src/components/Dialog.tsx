import type { ReactElement, ReactNode } from 'react';
import { Button } from './Button';
import styles from './Dialog.module.css';
import { Overlay } from './Overlay';

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
    <Overlay
      label={title}
      onClose={onCancel}
      overlayClassName={styles.overlay}
      panelClassName={styles.panel}
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
    </Overlay>
  );
}
