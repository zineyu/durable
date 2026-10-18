import { useState } from 'react';
import type { ReactNode } from 'react';
import styles from './WorkspaceGroup.module.css';

import type { WorkspacePath } from '../view/ids';

export type WorkspaceGroupProps = {
  path: WorkspacePath;
  sessionCount: number;
  children: ReactNode;
  defaultOpen?: boolean;
};

export function WorkspaceGroup({
  path,
  sessionCount,
  children,
  defaultOpen = true,
}: WorkspaceGroupProps) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <section className={styles.group}>
      <button
        type="button"
        className={styles.header}
        onClick={() => setOpen((current) => !current)}
        aria-expanded={open}
      >
        <span className={open ? styles.chevronOpen : styles.chevron} aria-hidden="true">
          ›
        </span>
        <span className={styles.path} title={path}>
          {path}
        </span>
        <span className={styles.count}>{sessionCount}</span>
      </button>
      {open ? <div className={styles.sessions}>{children}</div> : null}
    </section>
  );
}
