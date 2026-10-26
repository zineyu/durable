import type { ReactElement } from 'react';
import { ActivityPanel } from './ActivityPanel';
import { Button } from './Button';
import { NodeSwitcher } from './NodeSwitcher';
import { SessionListItem } from './SessionListItem';
import { WorkspaceGroup } from './WorkspaceGroup';
import styles from './Sidebar.module.css';
import type { SessionItemView } from './SessionListItem';
import type { NodeName } from '../api/generated/NodeName';
import type { SessionId, WorkspacePath } from '../view/ids';
import type { NodeRecord, RunningEntry, WorkspaceSummary } from '../view/types';

export type SessionEditing = {
  sessionId: SessionId;
  view: Exclude<SessionItemView, 'default'>;
};

export type SidebarProps = {
  nodes: NodeRecord[];
  selectedNode: NodeName | null;
  onSelectNode?: (node: NodeName) => void;
  onRegisterNode?: () => void;
  onRenameNode?: (node: NodeName) => void;
  onRemoveNode?: (node: NodeName) => void;
  /// 节点不可用或未知时的说明，替换空列表提示。
  notice?: string | null;
  commandsDisabled?: boolean;
  running?: RunningEntry[];
  onSelectRunning?: (node: NodeName, sessionId: SessionId) => void;
  onCancelRunning?: (node: NodeName, sessionId: SessionId) => void;
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
  onCreateSession?: (path: WorkspacePath) => void;
  onAddWorkspace?: () => void;
  onRemoveWorkspace?: (path: WorkspacePath) => void;
};

export function Sidebar({
  nodes,
  selectedNode,
  onSelectNode,
  onRegisterNode,
  onRenameNode,
  onRemoveNode,
  notice = null,
  commandsDisabled = false,
  running = [],
  onSelectRunning,
  onCancelRunning,
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
      <NodeSwitcher
        nodes={nodes}
        selected={selectedNode}
        {...(onSelectNode ? { onSelect: onSelectNode } : {})}
        {...(onRegisterNode ? { onRegister: onRegisterNode } : {})}
        {...(onRenameNode ? { onRename: onRenameNode } : {})}
        {...(onRemoveNode ? { onRemove: onRemoveNode } : {})}
      />
      <ActivityPanel
        entries={running}
        {...(onSelectRunning ? { onSelect: onSelectRunning } : {})}
        {...(onCancelRunning ? { onCancel: onCancelRunning } : {})}
      />
      <div className={styles.list}>
        {notice !== null ? (
          <p className={styles.notice}>{notice}</p>
        ) : workspaces.length === 0 ? (
          <p className={styles.empty}>还没有 workspace。注册一个目录后即可创建 session。</p>
        ) : null}
        {workspaces.map((workspace) => (
          <WorkspaceGroup
            key={workspace.id}
            path={workspace.path}
            sessionCount={workspace.sessions.length}
            {...(onCreateSession ? { onCreateSession: () => { onCreateSession(workspace.path); } } : {})}
            {...(onRemoveWorkspace ? { onRemove: () => { onRemoveWorkspace(workspace.path); } } : {})}
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
        <Button
          variant="secondary"
          onClick={onAddWorkspace}
          className={styles.addButton}
          disabled={commandsDisabled}
        >
          注册 workspace
        </Button>
      </footer>
    </div>
  );
}
