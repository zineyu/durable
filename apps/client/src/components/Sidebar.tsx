import type { ReactElement } from 'react';
import { ActivityPanel } from './ActivityPanel';
import { Button } from './Button';
import { HostSwitcher } from './HostSwitcher';
import { SessionListItem } from './SessionListItem';
import { WorkspaceGroup } from './WorkspaceGroup';
import styles from './Sidebar.module.css';
import type { SessionItemView } from './SessionListItem';
import type { HostName } from '../api/generated/HostName';
import type { SessionId, WorkspacePath } from '../view/ids';
import type {
  HostView,
  RunningEntry,
  SessionSummary,
  WorkspaceSummary,
} from '../view/types';

export type SessionEditing = {
  sessionId: SessionId;
  view: Exclude<SessionItemView, 'default'>;
};

export type SidebarProps = {
  hosts: HostView[];
  selectedHost: HostName | null;
  selectedSessionId: SessionId | null;
  /// host 切换器里按 host 分组的 session，顺序沿用侧栏。
  sessions: SessionSummary[];
  onSelectHostSession?: (sessionId: SessionId) => void;
  onRegisterHost?: () => void;
  onRenameHost?: (host: HostName) => void;
  onRemoveHost?: (host: HostName) => void;
  onToggleHost?: (host: HostView) => void;
  onReconnectHost?: (host: HostName) => void;
  /// host 不可用时禁用注册 workspace 的入口。
  registerDisabled?: boolean;
  running?: RunningEntry[];
  onSelectRunning?: (sessionId: SessionId) => void;
  onCancelRunning?: (sessionId: SessionId) => void;
  modelName: string;
  workspaces: WorkspaceSummary[];
  editing?: SessionEditing | null;
  onSelectSession?: (id: SessionId) => void;
  onRenameRequest?: (id: SessionId) => void;
  onRenameSubmit?: (id: SessionId, title: string) => void;
  onEditCancel?: () => void;
  onDeleteRequest?: (id: SessionId) => void;
  onDeleteConfirm?: (id: SessionId) => void;
  onCreateSession?: (host: string, path: WorkspacePath) => void;
  onAddWorkspace?: () => void;
  onRemoveWorkspace?: (host: string, path: WorkspacePath) => void;
};

export function Sidebar({
  hosts,
  selectedHost,
  selectedSessionId,
  sessions,
  onSelectHostSession,
  onRegisterHost,
  onRenameHost,
  onRemoveHost,
  onToggleHost,
  onReconnectHost,
  registerDisabled = false,
  running = [],
  onSelectRunning,
  onCancelRunning,
  modelName,
  workspaces,
  editing,
  onSelectSession,
  onRenameRequest,
  onRenameSubmit,
  onEditCancel,
  onDeleteRequest,
  onDeleteConfirm,
  onCreateSession,
  onAddWorkspace,
  onRemoveWorkspace,
}: SidebarProps): ReactElement {
  return (
    <div className={styles.sidebar}>
      <header className={styles.brand}>
        <span className={styles.brandName}>durable</span>
        <span className={styles.model}>{modelName}</span>
      </header>
      <HostSwitcher
        hosts={hosts}
        selectedHost={selectedHost}
        selectedSessionId={selectedSessionId}
        sessions={sessions}
        {...(onSelectHostSession ? { onSelectSession: onSelectHostSession } : {})}
        {...(onRegisterHost ? { onRegister: onRegisterHost } : {})}
        {...(onRenameHost ? { onRename: onRenameHost } : {})}
        {...(onRemoveHost ? { onRemove: onRemoveHost } : {})}
        {...(onToggleHost ? { onToggleEnabled: onToggleHost } : {})}
        {...(onReconnectHost ? { onReconnect: onReconnectHost } : {})}
      />
      <ActivityPanel
        entries={running}
        {...(onSelectRunning ? { onSelect: onSelectRunning } : {})}
        {...(onCancelRunning ? { onCancel: onCancelRunning } : {})}
      />
      <div className={styles.list}>
        {workspaces.length === 0 ? (
          <p className={styles.empty}>还没有 workspace。注册一个目录后即可创建 session。</p>
        ) : null}
        {workspaces.map((workspace) => (
          <WorkspaceGroup
            key={workspace.id}
            host={workspace.host}
            path={workspace.path}
            sessionCount={workspace.sessions.length}
            writesDisabled={!workspace.available}
            {...(onCreateSession ? { onCreateSession: () => { onCreateSession(workspace.host, workspace.path); } } : {})}
            {...(onRemoveWorkspace ? { onRemove: () => { onRemoveWorkspace(workspace.host, workspace.path); } } : {})}
          >
            {workspace.sessions.length === 0 ? (
              workspace.available ? (
                <p className={styles.emptySessions}>暂无 session</p>
              ) : (
                <p className={styles.notice}>{workspace.notice}</p>
              )
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
        <Button
          variant="secondary"
          onClick={onAddWorkspace}
          className={styles.addButton}
          disabled={registerDisabled}
        >
          注册 workspace
        </Button>
      </footer>
    </div>
  );
}
