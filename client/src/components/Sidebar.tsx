import { Button } from './Button';
import { SessionListItem } from './SessionListItem';
import { WorkspaceGroup } from './WorkspaceGroup';
import styles from './Sidebar.module.css';
import type { SessionId } from '../view/ids';
import type { WorkspaceSummary } from '../view/types';

export type SidebarProps = {
  modelName: string;
  workspaces: WorkspaceSummary[];
  selectedSessionId: SessionId | null;
  renamingSessionId?: SessionId | null;
  pendingDeleteSessionId?: SessionId | null;
  onSelectSession?: (id: SessionId) => void;
  onRenameRequest?: (id: SessionId) => void;
  onRenameSubmit?: (id: SessionId, title: string) => void;
  onRenameCancel?: () => void;
  onDeleteRequest?: (id: SessionId) => void;
  onDeleteConfirm?: (id: SessionId) => void;
  onDeleteCancel?: () => void;
  onAddWorkspace?: () => void;
};

export function Sidebar({
  modelName,
  workspaces,
  selectedSessionId,
  renamingSessionId,
  pendingDeleteSessionId,
  onSelectSession,
  onRenameRequest,
  onRenameSubmit,
  onRenameCancel,
  onDeleteRequest,
  onDeleteConfirm,
  onDeleteCancel,
  onAddWorkspace,
}: SidebarProps) {
  return (
    <div className={styles.sidebar}>
      <header className={styles.brand}>
        <span className={styles.brandName}>durable</span>
        <span className={styles.model}>{modelName}</span>
      </header>
      <div className={styles.list}>
        {workspaces.length === 0 ? (
          <p className={styles.empty}>还没有 workspace。注册一个目录后即可创建 session。</p>
        ) : null}
        {workspaces.map((workspace) => (
          <WorkspaceGroup
            key={workspace.id}
            path={workspace.path}
            sessionCount={workspace.sessions.length}
          >
            {workspace.sessions.length === 0 ? (
              <p className={styles.emptySessions}>暂无 session</p>
            ) : null}
            {workspace.sessions.map((session) => (
              <SessionListItem
                key={session.id}
                session={session}
                selected={session.id === selectedSessionId}
                renaming={session.id === renamingSessionId}
                confirmDelete={session.id === pendingDeleteSessionId}
                {...(onSelectSession ? { onSelect: onSelectSession } : {})}
                {...(onRenameRequest ? { onRenameRequest } : {})}
                {...(onRenameSubmit ? { onRenameSubmit } : {})}
                {...(onRenameCancel ? { onRenameCancel } : {})}
                {...(onDeleteRequest ? { onDeleteRequest } : {})}
                {...(onDeleteConfirm ? { onDeleteConfirm } : {})}
                {...(onDeleteCancel ? { onDeleteCancel } : {})}
              />
            ))}
          </WorkspaceGroup>
        ))}
      </div>
      <footer className={styles.footer}>
        <Button variant="secondary" onClick={onAddWorkspace} className={styles.addButton}>
          注册 workspace
        </Button>
      </footer>
    </div>
  );
}
