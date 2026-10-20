import styles from './MainHeader.module.css';
import { ConnectionChip } from './ConnectionChip';
import { NodeStatusBadge } from './NodeStatusBadge';
import type { NodeName } from '../api/generated/NodeName';
import type { NodeStatus } from '../api/generated/NodeStatus';
import { sessionStateLabel } from '../view/labels';
import type { WorkspacePath } from '../view/ids';
import type { ConnectionState, SessionState } from '../view/types';

export type MainHeaderProps = {
  node: NodeName;
  nodeStatus: NodeStatus;
  nodeDetail?: string | null;
  title: string;
  workspacePath: WorkspacePath;
  state: SessionState;
  connection: ConnectionState;
};

export function MainHeader({
  node,
  nodeStatus,
  nodeDetail = null,
  title,
  workspacePath,
  state,
  connection,
}: MainHeaderProps) {
  return (
    <header className={styles.header}>
      <div className={styles.inner}>
        <div className={styles.identity}>
          <h1 className={styles.title} title={title}>
            {title}
          </h1>
          <p className={styles.workspace} title={workspacePath}>
            <span className={styles.node}>{node}</span>
            <span className={styles.separator} aria-hidden="true">
              ·
            </span>
            <span className={styles.path}>{workspacePath}</span>
          </p>
        </div>
        <div className={styles.status}>
          <NodeStatusBadge status={nodeStatus} detail={nodeDetail} />
          <span className={styles.state} data-state={state}>
            {sessionStateLabel[state]}
          </span>
          <ConnectionChip state={connection} />
        </div>
      </div>
    </header>
  );
}
