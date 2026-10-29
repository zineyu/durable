import type { ReactElement } from 'react';
import styles from './MainHeader.module.css';
import { ConnectionChip } from './ConnectionChip';
import { IconButton } from './IconButton';
import { HostStatusBadge } from './HostStatusBadge';
import type { HostName } from '../api/generated/HostName';
import type { HostStatus } from '../api/generated/HostStatus';
import { sessionStateLabel } from '../view/labels';
import type { WorkspacePath } from '../view/ids';
import type { ConnectionState, SessionState } from '../view/types';

export type MainHeaderProps = {
  host: HostName;
  hostStatus: HostStatus;
  hostDetail?: string | null;
  title: string;
  workspacePath: WorkspacePath;
  state: SessionState;
  connection: ConnectionState;
  onSearch?: () => void;
  onExport?: () => void;
  searchOpen?: boolean;
};

export function MainHeader({
  host,
  hostStatus,
  hostDetail = null,
  title,
  workspacePath,
  state,
  connection,
  onSearch,
  onExport,
  searchOpen = false,
}: MainHeaderProps): ReactElement {
  const hasActions = onSearch !== undefined || onExport !== undefined;
  return (
    <header className={styles.header}>
      <div className={styles.inner}>
        <div className={styles.identity}>
          <h1 className={styles.title} title={title}>
            {title}
          </h1>
          <p className={styles.workspace} title={workspacePath}>
            <span className={styles.host}>{host}</span>
            <span className={styles.separator} aria-hidden="true">
              ·
            </span>
            <span className={styles.path}>{workspacePath}</span>
          </p>
        </div>
        <div className={styles.status}>
          {hasActions ? (
            <span className={styles.actions}>
              {onSearch !== undefined ? (
                <IconButton
                  icon="search"
                  label="搜索消息"
                  aria-pressed={searchOpen}
                  onClick={onSearch}
                />
              ) : null}
              {onExport !== undefined ? (
                <IconButton icon="download" label="导出 Markdown" onClick={onExport} />
              ) : null}
            </span>
          ) : null}
          <HostStatusBadge status={hostStatus} detail={hostDetail} />
          <span className={styles.state} data-state={state}>
            {sessionStateLabel[state]}
          </span>
          <ConnectionChip state={connection} />
        </div>
      </div>
    </header>
  );
}
