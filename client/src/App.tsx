import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { ReactElement } from 'react';
import { api } from './api/client';
import type { HostName } from './api/generated/HostName';
import { AppShell } from './components/AppShell';
import { CommandPalette } from './components/CommandPalette';
import { Dialog } from './components/Dialog';
import {
  dialogConfirmDisabled,
  dialogConfirmLabel,
  dialogTitle,
  renderDialogBody,
} from './components/dialog-body';
import { ErrorBanner } from './components/ErrorBanner';
import { MainPane } from './components/MainPane';
import { ShortcutHelp } from './components/ShortcutHelp';
import { Sidebar } from './components/Sidebar';
import { buildPalette, filterPalette, stepPaletteIndex } from './view/command-palette';
import type { PaletteEntry } from './view/command-palette';
import {
  hasSession,
  orderedSessions,
  paletteSessions,
  runningEntries,
  sessionView,
  workspaceSummaries,
} from './view/derived';
import { readLastSession, writeLastSession } from './view/drafts';
import { hostNotice } from './view/labels';
import { readSelection, writeSelection } from './view/session-hash';
import type { Selection } from './view/session-hash';
import { isEditableTarget, resolveShortcut } from './view/shortcuts';
import { useClientState } from './view/use-client-state';
import { useCommands } from './view/use-commands';
import { useDocumentTitle } from './view/use-document-title';

const storage = window.localStorage;

/// 本模块负责会话选择位置、快捷键、命令面板与侧栏装配。写命令在 `view/use-commands.ts`，
/// 主面板分支在 `components/MainPane.tsx`，派生视图在 `view/derived.ts`。
/// 选中位置只是一个 session id；host 由该 session 的 workspace 推出。
/// 规格 user story R-45：刷新后恢复上次打开的 session。
export function App(): ReactElement {
  const [selection, setSelection] = useState<Selection>(() => readSelection(window.location.hash));
  const { state, dispatch, report, runCommand } = useClientState(selection);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [paletteQuery, setPaletteQuery] = useState('');
  const [paletteIndex, setPaletteIndex] = useState(0);
  const [helpOpen, setHelpOpen] = useState(false);

  const view = useMemo(
    () => (selection === null ? null : sessionView(state, selection)),
    [selection, state],
  );
  const workspaces = useMemo(() => workspaceSummaries(state), [state]);
  const sessions = useMemo(() => orderedSessions(workspaces), [workspaces]);
  const host = useMemo(
    () => (view === null ? undefined : state.hosts.find((entry) => entry.name === view.session.host)),
    [state.hosts, view],
  );
  const selectedHost = host?.name ?? null;
  const connected = host === undefined || host.status === 'ok';
  const notice = selection === null ? null : hostNotice(host);

  const navigate = useCallback((sessionId: Selection) => {
    window.location.hash = writeSelection(sessionId);
  }, []);

  const commands = useCommands({ selection, view, dispatch, report, runCommand, navigate });
  const { cancelRun, closeDialog, dialog, resetForLocation } = commands;

  const restoreLast = useRef(false);

  useEffect(() => {
    if (restoreLast.current || state.lastEventId === null) {
      return;
    }
    restoreLast.current = true;
    if (selection !== null) {
      return;
    }
    const last = readLastSession(storage);
    if (last !== null && hasSession(state, last)) {
      navigate(last);
    }
  }, [navigate, selection, state]);

  useEffect(() => {
    const apply = () => {
      const next = readSelection(window.location.hash);
      setSelection(next);
      resetForLocation(next);
      setPaletteOpen(false);
    };
    window.addEventListener('hashchange', apply);
    return () => { window.removeEventListener('hashchange', apply); };
  }, [resetForLocation]);

  useEffect(() => {
    if (selection !== null) {
      writeLastSession(storage, selection);
    }
  }, [selection]);

  useEffect(() => {
    if (view === null) {
      return;
    }
    dispatch({
      type: 'seen',
      sessionId: view.summary.id,
      at: view.summary.updatedAt,
    });
  }, [dispatch, view]);

  useDocumentTitle(view);

  const paletteEntries = useMemo(
    () =>
      filterPalette(
        buildPalette({
          hosts: state.hosts,
          sessions: paletteSessions(state),
          selectedHost,
          selectedSession: view === null ? null : view.summary,
        }),
        paletteQuery,
      ),
    [paletteQuery, selectedHost, state, view],
  );
  const paletteCursor =
    paletteEntries.length === 0 ? 0 : Math.min(paletteIndex, paletteEntries.length - 1);

  const selectSessionOfHost = useCallback(
    (name: HostName) => {
      const candidates = paletteSessions(state)
        .filter((session) => session.host === name)
        .sort((left, right) => right.updatedAt - left.updatedAt);
      const target = candidates[0];
      if (target !== undefined) {
        navigate(target.sessionId);
      }
    },
    [navigate, state],
  );

  const runPaletteEntry = useCallback(
    (entry: PaletteEntry) => {
      setPaletteOpen(false);
      switch (entry.action.kind) {
        case 'session':
          navigate(entry.action.sessionId);
          return;
        case 'host':
          selectSessionOfHost(entry.action.host);
          return;
        case 'register-host':
          commands.setDialog({ kind: 'register-host', name: '', hostKind: 'local', target: '' });
          return;
        case 'register-workspace':
          commands.setDialog({ kind: 'register-workspace', host: selectedHost ?? 'local', path: '' });
          return;
        case 'cancel-run':
          runCommand(api.cancelRun(entry.action.sessionId));
          return;
        default: {
          const unhandled: never = entry.action;
          throw new Error(`未处理的命令面板动作: ${JSON.stringify(unhandled)}`);
        }
      }
    },
    [commands, navigate, runCommand, selectSessionOfHost, selectedHost],
  );

  const cycleSession = useCallback(
    (delta: number) => {
      if (sessions.length === 0) {
        return;
      }
      const current = sessions.findIndex((entry) => entry.id === selection);
      const next =
        current === -1
          ? delta > 0
            ? 0
            : sessions.length - 1
          : (current + delta + sessions.length) % sessions.length;
      const target = sessions[next];
      if (target !== undefined) {
        navigate(target.id);
      }
    },
    [navigate, selection, sessions],
  );

  useEffect(() => {
    const handle = (event: KeyboardEvent) => {
      const command = resolveShortcut(event, { editable: isEditableTarget(event.target) });
      if (command === null) {
        return;
      }
      if (command.type === 'open-palette') {
        event.preventDefault();
        if (paletteOpen) {
          setPaletteOpen(false);
        } else {
          setPaletteQuery('');
          setPaletteIndex(0);
          setPaletteOpen(true);
        }
        return;
      }
      if (command.type === 'escape') {
        if (paletteOpen) {
          setPaletteOpen(false);
          return;
        }
        if (helpOpen) {
          setHelpOpen(false);
          return;
        }
        if (dialog !== null) {
          closeDialog();
          return;
        }
        cancelRun();
        return;
      }
      if (paletteOpen || helpOpen) {
        return;
      }
      switch (command.type) {
        case 'toggle-help':
          event.preventDefault();
          setHelpOpen(true);
          return;
        case 'cycle-session':
          event.preventDefault();
          cycleSession(command.delta);
          return;
        case 'select-host': {
          const target = state.hosts[command.index];
          if (target !== undefined) {
            event.preventDefault();
            selectSessionOfHost(target.name);
          }
          return;
        }
        default:
          return;
      }
    };
    window.addEventListener('keydown', handle);
    return () => { window.removeEventListener('keydown', handle); };
  }, [
    cancelRun,
    closeDialog,
    cycleSession,
    dialog,
    helpOpen,
    paletteOpen,
    selectSessionOfHost,
    state.hosts,
  ]);

  const running = useMemo(() => runningEntries(state), [state]);

  return (
    <>
      <AppShell
        sidebar={
          <Sidebar
            hosts={state.hosts}
            selectedHost={selectedHost}
            selectedSessionId={selection}
            sessions={sessions}
            onSelectHostSession={navigate}
            onRegisterHost={() => {
              commands.setDialog({ kind: 'register-host', name: '', hostKind: 'local', target: '' });
            }}
            onRenameHost={(name) => {
              commands.setDialog({ kind: 'rename-host', name, next: name });
            }}
            onRemoveHost={(name) => {
              commands.setDialog({ kind: 'remove-host', name });
            }}
            onToggleHost={(record) => {
              runCommand(api.updateHost(record.name, { enabled: !record.enabled }));
            }}
            onReconnectHost={(name) => { runCommand(api.reconnectHost(name)); }}
            notice={notice}
            commandsDisabled={!connected}
            running={running}
            onSelectRunning={navigate}
            onCancelRunning={(id) => { runCommand(api.cancelRun(id)); }}
            modelName={state.modelName}
            workspaces={workspaces}
            editing={commands.editing}
            onSelectSession={navigate}
            onRenameRequest={commands.requestRename}
            onRenameSubmit={commands.renameSession}
            onDeleteRequest={commands.requestDelete}
            onDeleteConfirm={commands.deleteSession}
            onEditCancel={commands.cancelEditing}
            onCreateSession={commands.createSession}
            onAddWorkspace={() => {
              commands.setDialog({ kind: 'register-workspace', host: selectedHost ?? 'local', path: '' });
            }}
            onRemoveWorkspace={(host, path) => {
              commands.setDialog({ kind: 'remove-workspace', host, path });
            }}
          />
        }
      >
        {state.error !== null ? (
          <ErrorBanner
            code={state.error.code}
            message={state.error.message}
            onDismiss={() => { dispatch({ type: 'dismiss-error' }); }}
          />
        ) : null}
        <MainPane
          sessionId={selection}
          view={view}
          host={host}
          connection={state.connection}
          draft={commands.draft.text}
          pastes={commands.draft.pastes}
          onDraftChange={commands.changeDraft}
          onSend={commands.send}
          onCancelRun={cancelRun}
          sendMode={commands.sendMode}
          onModeChange={commands.changeMode}
          runCommand={runCommand}
          onEditMessage={(messageId, text) => {
            if (view !== null) {
              commands.setDialog({
                kind: 'edit',
                sessionId: view.session.id,
                messageId,
                text,
              });
            }
          }}
          onRetryMessage={(messageId) => {
            if (view !== null) {
              commands.setDialog({ kind: 'retry', sessionId: view.session.id, messageId });
            }
          }}
          onRegisterWorkspace={() => {
            commands.setDialog({ kind: 'register-workspace', host: selectedHost ?? 'local', path: '' });
          }}
        />
      </AppShell>
      {commands.dialog !== null ? (
        <Dialog
          title={dialogTitle(commands.dialog)}
          confirmLabel={dialogConfirmLabel(commands.dialog)}
          confirmDisabled={dialogConfirmDisabled(commands.dialog)}
          danger={commands.dialog.kind === 'remove-host'}
          onConfirm={commands.confirmDialog}
          onCancel={closeDialog}
        >
          {renderDialogBody(commands.dialog, commands.setDialog)}
        </Dialog>
      ) : null}
      {paletteOpen ? (
        <CommandPalette
          entries={paletteEntries}
          selectedIndex={paletteCursor}
          query={paletteQuery}
          onQueryChange={(next) => {
            setPaletteQuery(next);
            setPaletteIndex(0);
          }}
          onMove={(delta) => {
            setPaletteIndex(stepPaletteIndex(paletteCursor, delta, paletteEntries.length));
          }}
          onHover={setPaletteIndex}
          onSelect={runPaletteEntry}
          onClose={() => { setPaletteOpen(false); }}
        />
      ) : null}
      {helpOpen ? <ShortcutHelp onClose={() => { setHelpOpen(false); }} /> : null}
    </>
  );
}
