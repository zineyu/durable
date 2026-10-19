import { Button } from './Button';
import { SessionListItem } from './SessionListItem';
import { WorkspaceGroup } from './WorkspaceGroup';
import styles from './Sidebar.module.css';
import type { SessionItemView } from './SessionListItem';
import type { SessionId } from '../view/ids';
import type { WorkspaceSummary } from '../view/types';

export type SessionEditing = {
  sessionId: SessionId;
  view: Exclude<SessionItemView, 'default'>;
};

export type SidebarProps = {
  modelName: string;
  workspaces: WorkspaceSummary[];
  selectedSessionId: SessionId | null;
  editing?: SessionEditing | null;
  onSelectSession?: (id: SessionId) => void;
  onRenameRequest?: (id: SessionId) => void;
  onRenameSubmit?: (id: SessionId, title: string) => void;
  onEditCancel?: () => void;
  onDeleteRequest?: (id: SessionId) => void;
  onDeleteConfirm?: (id: SessionId) => void;
  onAddWorkspace?: () => void;
};

export function Sidebar({
  modelName,
  workspaces,
  selectedSessionId,
  editing,
  onSelectSession,
  onRenameRequest,
  onRenameSubmit,
  onEditCancel,
  onDeleteRequest,
  onDeleteConfirm,
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
                view={editing?.sessionId === session.id ? editing.view : 'default'}
                {...(onSelectSession ? { onSelect: onSelectSession } : {})}
                {...(onRenameRequest ? { onRenameRequest } : {})}
                {...(onRenameSubmit ? { onRenameSubmit } : {})}
                {...(onEditCancel ? { onEditCancel } : {})}
                {...(onDeleteRequest ? { onDeleteRequest } : {})}
                {...(onDeleteConfirm ? { onDeleteConfirm } : {})}
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
