import type { ReactElement } from 'react';
import { IconButton } from './IconButton';
import styles from './ErrorBanner.module.css';

export type ErrorBannerProps = {
  code: string;
  message: string;
  onDismiss?: () => void;
};

export function ErrorBanner({ code, message, onDismiss }: ErrorBannerProps): ReactElement {
  return (
    <div className={styles.banner} role="alert">
      <span className={styles.code}>{code}</span>
      <span className={styles.message}>{message}</span>
      <IconButton icon="close" label="关闭" tone="danger" onClick={onDismiss} />
    </div>
  );
}
