import type { ReactElement, ReactNode } from 'react';
import styles from './AppShell.module.css';

export type AppShellProps = {
  sidebar: ReactNode;
  children: ReactNode;
};

export function AppShell({ sidebar, children }: AppShellProps): ReactElement {
  return (
    <div className={styles.shell}>
      <aside className={styles.sidebar}>{sidebar}</aside>
      <main className={styles.main}>{children}</main>
    </div>
  );
}
