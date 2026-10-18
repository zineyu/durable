import styles from './MainHeader.module.css';
import { ConnectionChip } from './ConnectionChip';
import { sessionStateLabel } from '../view/labels';
import type { WorkspacePath } from '../view/ids';
import type { ConnectionState, SessionState } from '../view/types';

export type MainHeaderProps = {
  title: string;
  workspacePath: WorkspacePath;
  state: SessionState;
  connection: ConnectionState;
};

export function MainHeader({ title, workspacePath, state, connection }: MainHeaderProps) {
  return (
    <header className={styles.header}>
      <div className={styles.inner}>
        <div className={styles.identity}>
          <h1 className={styles.title} title={title}>
            {title}
          </h1>
          <p className={styles.workspace} title={workspacePath}>
            {workspacePath}
          </p>
        </div>
        <div className={styles.status}>
          <span className={styles.state} data-state={state}>
            {sessionStateLabel[state]}
          </span>
          <ConnectionChip state={connection} />
        </div>
      </div>
    </header>
  );
}
