import { useCallback, useEffect, useReducer, useState } from 'react';
import type { ReactNode } from 'react';
import { api } from './api/client';
import type { ApiFailure, ApiResult } from './api/client';
import { openEventStream } from './api/events';
import type { EventStream } from './api/events';
import type { MessageNode } from './api/generated/MessageNode';
import { AppShell } from './components/AppShell';
import { AssistantContent } from './components/AssistantContent';
import { AssistantMessage } from './components/AssistantMessage';
import { Composer } from './components/Composer';
import { Dialog } from './components/Dialog';
import { EmptyState } from './components/EmptyState';
import { ErrorBanner } from './components/ErrorBanner';
import { MainHeader } from './components/MainHeader';
import { QueueStrip } from './components/QueueStrip';
import { Sidebar } from './components/Sidebar';
import type { SessionEditing } from './components/Sidebar';
import { Transcript } from './components/Transcript';
import { UserMessage } from './components/UserMessage';
import { toQueueItemId, toSessionId, toWorkspacePath } from './view/ids';
import type { SessionId, WorkspacePath } from './view/ids';
import {
  branchPosition,
  initialClientState,
  messageView,
  reduce,
  sessionQueue,
  sidebarWorkspaces,
  transcriptNodes,
} from './view/reducer';
import { readSessionFromHash, writeSessionToHash } from './view/session-hash';
import type { BranchControl, SessionState } from './view/types';

type DialogState =
  | { kind: 'edit'; sessionId: string; messageId: number; text: string }
  | { kind: 'retry'; sessionId: string; messageId: number }
  | { kind: 'register-workspace'; path: string };

function sessionStateOf(busy: boolean, queueLength: number): SessionState {
  if (busy) {
    return 'running';
  }
  return queueLength > 0 ? 'queued' : 'idle';
}

export function App() {
  const [state, dispatch] = useReducer(reduce, initialClientState);
  const [selectedSessionId, setSelectedSessionId] = useState<SessionId | null>(() =>
    readSessionFromHash(window.location.hash),
  );
  const [editing, setEditing] = useState<SessionEditing | null>(null);
  const [dialog, setDialog] = useState<DialogState | null>(null);
  const [draft, setDraft] = useState('');

  const report = useCallback((failure: ApiFailure) => {
    dispatch({ type: 'error', error: { code: failure.code, message: failure.message } });
  }, []);

  const runCommand = useCallback(
    (result: Promise<ApiResult<unknown>>) => {
      void (async () => {
        const outcome = await result;
        if (!outcome.ok) {
          report(outcome.error);
        }
      })();
    },
    [report],
  );

  useEffect(() => {
    const handleHashChange = () => {
      setSelectedSessionId(readSessionFromHash(window.location.hash));
    };
    window.addEventListener('hashchange', handleHashChange);
    return () => window.removeEventListener('hashchange', handleHashChange);
  }, []);

  useEffect(() => {
    let active = true;
    let stream: EventStream | null = null;
    void (async () => {
      const outcome = await api.bootstrap();
      if (!active) {
        return;
      }
      if (!outcome.ok) {
        report(outcome.error);
        dispatch({ type: 'connection', connection: 'disconnected' });
        return;
      }
      dispatch({ type: 'bootstrap', bootstrap: outcome.value });
      stream = openEventStream(outcome.value.maxEventId, {
        onEvent: (event) => dispatch({ type: 'stream', event }),
        onState: (connection) => dispatch({ type: 'connection', connection }),
      });
    })();
    return () => {
      active = false;
      stream?.close();
    };
  }, [report]);

  const session =
    selectedSessionId === null
      ? undefined
      : state.sessions.find((candidate) => candidate.id === selectedSessionId);
  const sessionExists = session !== undefined;

  useEffect(() => {
    if (selectedSessionId === null || !sessionExists) {
      return;
    }
    const id = selectedSessionId;
    let active = true;
    void (async () => {
      const outcome = await api.sessionTree(id);
      if (!active) {
        return;
      }
      if (!outcome.ok) {
        report(outcome.error);
        return;
      }
      dispatch({ type: 'tree', tree: outcome.value });
    })();
    return () => {
      active = false;
    };
  }, [selectedSessionId, sessionExists, report]);

  const selectSession = useCallback((id: SessionId) => {
    window.location.hash = writeSessionToHash(id);
  }, []);

  const handleCreateSession = useCallback(
    (workspace: WorkspacePath) => {
      void (async () => {
        const outcome = await api.createSession(workspace, null);
        if (!outcome.ok) {
          report(outcome.error);
          return;
        }
        dispatch({ type: 'session', session: outcome.value });
        window.location.hash = writeSessionToHash(toSessionId(outcome.value.id));
      })();
    },
    [report],
  );

  const handleRenameSubmit = useCallback(
    (id: SessionId, title: string) => {
      void (async () => {
        const outcome = await api.renameSession(id, title);
        if (!outcome.ok) {
          report(outcome.error);
          return;
        }
        setEditing(null);
      })();
    },
    [report],
  );

  const handleDeleteConfirm = useCallback(
    (id: SessionId) => {
      void (async () => {
        const outcome = await api.deleteSession(id);
        if (!outcome.ok) {
          report(outcome.error);
          return;
        }
        setEditing(null);
        if (id === selectedSessionId) {
          window.location.hash = '';
        }
      })();
    },
    [report, selectedSessionId],
  );

  const handleSend = useCallback(() => {
    if (session === undefined) {
      return;
    }
    const text = draft.trim();
    if (text.length === 0) {
      return;
    }
    const id = session.id;
    void (async () => {
      const outcome = await api.submitMessage(id, text);
      if (!outcome.ok) {
        report(outcome.error);
        return;
      }
      setDraft('');
    })();
  }, [draft, session, report]);

  const handleCancelRun = useCallback(() => {
    if (session === undefined) {
      return;
    }
    runCommand(api.cancelRun(session.id));
  }, [session, runCommand]);

  const handleDialogConfirm = useCallback(() => {
    if (dialog === null) {
      return;
    }
    const current = dialog;
    void (async () => {
      let outcome: ApiResult<unknown>;
      switch (current.kind) {
        case 'edit':
          outcome = await api.editUserMessage(current.sessionId, current.messageId, current.text);
          break;
        case 'retry':
          outcome = await api.retryAssistantMessage(current.sessionId, current.messageId);
          break;
        case 'register-workspace':
          outcome = await api.registerWorkspace(current.path);
          break;
        default: {
          const unhandled: never = current;
          throw new Error(`未处理的对话框: ${JSON.stringify(unhandled)}`);
        }
      }
      if (!outcome.ok) {
        report(outcome.error);
        return;
      }
      setDialog(null);
    })();
  }, [dialog, report]);

  const branchControl = useCallback(
    (sessionId: string, node: MessageNode): { branch?: BranchControl } => {
      const position = branchPosition(state, sessionId, node);
      if (position === null) {
        return {};
      }
      const control: BranchControl = { index: position.index, total: position.total };
      const previousId = position.previousId;
      const nextId = position.nextId;
      if (previousId !== null) {
        control.onPrev = () => runCommand(api.switchActiveLeaf(sessionId, previousId));
      }
      if (nextId !== null) {
        control.onNext = () => runCommand(api.switchActiveLeaf(sessionId, nextId));
      }
      return { branch: control };
    },
    [state, runCommand],
  );

  const workspaces = sidebarWorkspaces(state);
  const isIdle = session !== undefined && !session.busy && session.queueLength === 0;
  const queue =
    session === undefined
      ? []
      : sessionQueue(state, session.id).map((item) => ({
          id: toQueueItemId(String(item.id)),
          text: item.text,
        }));

  const entries =
    session === undefined
      ? []
      : transcriptNodes(state, session.id).map((node) => {
          if (node.role === 'user') {
            return (
              <UserMessage
                key={node.id}
                text={node.text}
                onEdit={() =>
                  setDialog({
                    kind: 'edit',
                    sessionId: session.id,
                    messageId: node.id,
                    text: node.text,
                  })
                }
                editDisabled={!isIdle}
                {...branchControl(session.id, node)}
              />
            );
          }
          const view = messageView(state, session.id, node);
          return (
            <AssistantMessage
              key={node.id}
              state={view.state}
              {...(view.note === null ? {} : { note: view.note })}
              onRetry={() =>
                setDialog({ kind: 'retry', sessionId: session.id, messageId: node.id })
              }
              retryDisabled={!isIdle}
              {...branchControl(session.id, node)}
            >
              <AssistantContent text={node.text} toolCalls={node.toolCalls} />
            </AssistantMessage>
          );
        });

  return (
    <>
      <AppShell
        sidebar={
          <Sidebar
            modelName={state.modelName}
            workspaces={workspaces}
            selectedSessionId={selectedSessionId}
            editing={editing}
            onSelectSession={selectSession}
            onRenameRequest={(id) => setEditing({ sessionId: id, view: 'renaming' })}
            onRenameSubmit={handleRenameSubmit}
            onDeleteRequest={(id) => setEditing({ sessionId: id, view: 'deleting' })}
            onDeleteConfirm={handleDeleteConfirm}
            onEditCancel={() => setEditing(null)}
            onCreateSession={handleCreateSession}
            onAddWorkspace={() => setDialog({ kind: 'register-workspace', path: '' })}
          />
        }
      >
        {state.error !== null ? (
          <ErrorBanner
            code={state.error.code}
            message={state.error.message}
            onDismiss={() => dispatch({ type: 'dismiss-error' })}
          />
        ) : null}
        {session === undefined ? (
          <EmptyState
            title="选择一个 session"
            description="从左侧选择 session，或者注册新的 workspace 后创建 session。"
            actionLabel="注册 workspace"
            onAction={() => setDialog({ kind: 'register-workspace', path: '' })}
          />
        ) : (
          <>
            <MainHeader
              title={session.title}
              workspacePath={toWorkspacePath(session.workspace)}
              state={sessionStateOf(session.busy, session.queueLength)}
              connection={state.connection}
            />
            <Transcript>
              {entries.length === 0 ? (
                <EmptyState title="还没有消息" description="在下方输入框提交第一条消息。" />
              ) : (
                entries
              )}
            </Transcript>
            <QueueStrip
              items={queue}
              onRemove={(viewId) => runCommand(api.removeQueueItem(session.id, Number(viewId)))}
            />
            <Composer
              value={draft}
              onChange={setDraft}
              onSend={handleSend}
              {...(session.busy ? { onCancel: handleCancelRun } : {})}
            />
          </>
        )}
      </AppShell>
      {dialog !== null ? (
        <Dialog
          title={dialogTitle(dialog)}
          confirmLabel={dialogConfirmLabel(dialog)}
          confirmDisabled={dialog.kind === 'register-workspace' && dialog.path.trim().length === 0}
          onConfirm={handleDialogConfirm}
          onCancel={() => setDialog(null)}
        >
          {renderDialogBody(dialog, setDialog)}
        </Dialog>
      ) : null}
    </>
  );
}

function dialogTitle(dialog: DialogState): string {
  switch (dialog.kind) {
    case 'edit':
      return '编辑用户消息';
    case 'retry':
      return '确认重试';
    case 'register-workspace':
      return '注册 workspace';
    default: {
      const unhandled: never = dialog;
      throw new Error(`未处理的对话框: ${JSON.stringify(unhandled)}`);
    }
  }
}

function dialogConfirmLabel(dialog: DialogState): string {
  switch (dialog.kind) {
    case 'edit':
      return '确认编辑';
    case 'retry':
      return '重试';
    case 'register-workspace':
      return '注册';
    default: {
      const unhandled: never = dialog;
      throw new Error(`未处理的对话框: ${JSON.stringify(unhandled)}`);
    }
  }
}

function renderDialogBody(
  dialog: DialogState,
  setDialog: (next: DialogState | null) => void,
): ReactNode {
  switch (dialog.kind) {
    case 'edit':
      return (
        <textarea
          value={dialog.text}
          aria-label="消息正文"
          rows={4}
          autoFocus
          onChange={(event) => setDialog({ ...dialog, text: event.target.value })}
        />
      );
    case 'retry':
      return (
        <p>
          重试以父用户消息为锚新建 run，产生同父的 assistant 兄弟节点。原回答保留，此操作不可撤销。
        </p>
      );
    case 'register-workspace':
      return (
        <input
          value={dialog.path}
          aria-label="workspace 路径"
          placeholder="/absolute/path"
          autoFocus
          onChange={(event) => setDialog({ ...dialog, path: event.target.value })}
        />
      );
    default: {
      const unhandled: never = dialog;
      throw new Error(`未处理的对话框: ${JSON.stringify(unhandled)}`);
    }
  }
}
