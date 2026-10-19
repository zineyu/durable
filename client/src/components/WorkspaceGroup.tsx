import { useState } from 'react';
import type { ReactNode } from 'react';
import { IconButton } from './IconButton';
import styles from './WorkspaceGroup.module.css';

import type { WorkspacePath } from '../view/ids';

export type WorkspaceGroupProps = {
  path: WorkspacePath;
  sessionCount: number;
  children: ReactNode;
  defaultOpen?: boolean;
  onCreateSession?: () => void;
};

export function WorkspaceGroup({
  path,
  sessionCount,
  children,
  defaultOpen = true,
  onCreateSession,
}: WorkspaceGroupProps) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <section className={styles.group}>
      <div className={styles.header}>
        <button
          type="button"
          className={styles.toggle}
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
        <IconButton
          icon="plus"
          label={`在 ${path} 下新建 session`}
          onClick={() => onCreateSession?.()}
        />
      </div>
      {open ? <div className={styles.sessions}>{children}</div> : null}
    </section>
  );
}
