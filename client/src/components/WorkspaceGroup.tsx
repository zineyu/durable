import { useState } from 'react';
import type { ReactElement, ReactNode } from 'react';
import { IconButton } from './IconButton';
import styles from './WorkspaceGroup.module.css';

import type { WorkspacePath } from '../view/ids';

export type WorkspaceGroupProps = {
  host: string;
  path: WorkspacePath;
  sessionCount: number;
  children: ReactNode;
  defaultOpen?: boolean;
  onCreateSession?: () => void;
  onRemove?: () => void;
};

export function WorkspaceGroup({
  host,
  path,
  sessionCount,
  children,
  defaultOpen = true,
  onCreateSession,
  onRemove,
}: WorkspaceGroupProps): ReactElement {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <section className={styles.group}>
      <div className={styles.header}>
        <button
          type="button"
          className={styles.toggle}
          onClick={() => { setOpen((current) => !current); }}
          aria-expanded={open}
        >
          <span className={open ? styles.chevronOpen : styles.chevron} aria-hidden="true">
            ›
          </span>
          <span className={styles.path} title={`${host}:${path}`}>
            {host} · {path}
          </span>
          <span className={styles.count}>{sessionCount}</span>
        </button>
        <IconButton
          icon="plus"
          label={`在 ${path} 下新建 session`}
          onClick={() => onCreateSession?.()}
        />
        <IconButton
          icon="trash"
          label={`移除 workspace：${path}`}
          tone="danger"
          className={styles.remove}
          onClick={() => onRemove?.()}
        />
      </div>
      {open ? <div className={styles.sessions}>{children}</div> : null}
    </section>
  );
}
