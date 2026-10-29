import { useCallback, useEffect, useMemo, useState } from 'react';
import type { ReactElement } from 'react';
import { api } from './api/client';
import type { NodeName } from './api/generated/NodeName';
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
  nodeSummary,
  orderedSessions,
  paletteSessions,
  runningEntries,
  sessionView,
} from './view/derived';
import { readLastSession, writeLastSession } from './view/drafts';
import type { SessionId } from './view/ids';
import { unavailableNotice } from './view/labels';
import { readSelection, writeLocationToHash } from './view/session-hash';
import { isEditableTarget, resolveShortcut } from './view/shortcuts';
import type { Selection } from './view/session-hash';
import { useClientState } from './view/use-client-state';
import { useCommands } from './view/use-commands';
import { useDocumentTitle } from './view/use-document-title';

const storage = window.localStorage;

/// 本模块负责会话选择位置、快捷键、命令面板与侧栏装配。写命令在 `view/use-commands.ts`，
/// 主面板分支在 `components/MainPane.tsx`，派生视图在 `view/derived.ts`。
export function App(): ReactElement {
  const [selection, setSelection] = useState<Selection>(() => readSelection(window.location.hash));
  const { state, dispatch, report, runCommand } = useClientState(selection);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [paletteQuery, setPaletteQuery] = useState('');
  const [paletteIndex, setPaletteIndex] = useState(0);
  const [helpOpen, setHelpOpen] = useState(false);

  const selectedNode = selection.node;
  const record =
    selectedNode === null
      ? undefined
      : state.hub.nodes.find((entry) => entry.name === selectedNode);
  const shard = selectedNode === null ? undefined : state.nodes[selectedNode];
  const view = useMemo(
    () =>
      shard === undefined || selection.sessionId === null
        ? null
        : sessionView(shard, selection.sessionId),
    [shard, selection.sessionId],
  );
  const summary = useMemo(() => (shard === undefined ? null : nodeSummary(shard)), [shard]);
  const nodeReady = record?.status === 'ok';

  const navigate = useCallback((node: NodeName | null, sessionId: SessionId | null) => {
    window.location.hash = node === null ? '' : writeLocationToHash(node, sessionId);
  }, []);

  const commands = useCommands({ selection, view, dispatch, report, runCommand, navigate });
  const { cancelRun, closeDialog, dialog, resetForLocation, setDialog } = commands;

  const selectNode = useCallback(
    (node: NodeName) => { navigate(node, null); },
    [navigate],
  );

  const selectSession = useCallback(
    (id: SessionId) => {
      if (selectedNode !== null) {
        navigate(selectedNode, id);
      }
    },
    [navigate, selectedNode],
  );

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
    if (selectedNode === null || selection.sessionId !== null) {
      return;
    }
    const current = state.hub.nodes.find((entry) => entry.name === selectedNode);
    if (current?.status !== 'ok' || shard === undefined) {
      return;
    }
    const last = readLastSession(storage, selectedNode);
    if (last === null || !hasSession(shard, last)) {
      return;
    }
    navigate(selectedNode, last);
  }, [navigate, selectedNode, selection.sessionId, shard, state.hub.nodes]);

  useEffect(() => {
    if (selectedNode !== null && selection.sessionId !== null) {
      writeLastSession(storage, selectedNode, selection.sessionId);
    }
  }, [selectedNode, selection.sessionId]);

  useEffect(() => {
    if (selectedNode === null || view === null) {
      return;
    }
    dispatch({
      type: 'seen',
      node: selectedNode,
      sessionId: view.summary.id,
      at: view.summary.updatedAt,
    });
  }, [dispatch, selectedNode, view]);

  useDocumentTitle(selectedNode, view);

  const paletteEntries = useMemo(
    () =>
      filterPalette(
        buildPalette({
          nodes: state.hub.nodes,
          sessions: paletteSessions(state.hub.nodes, state.nodes),
          selectedNode,
          selectedSession: view === null ? null : view.summary,
        }),
        paletteQuery,
      ),
    [paletteQuery, selectedNode, state.hub.nodes, state.nodes, view],
  );
  const paletteCursor =
    paletteEntries.length === 0 ? 0 : Math.min(paletteIndex, paletteEntries.length - 1);

  const runPaletteEntry = useCallback(
    (entry: PaletteEntry) => {
      setPaletteOpen(false);
      switch (entry.action.kind) {
        case 'session':
          navigate(entry.action.node, entry.action.sessionId);
          return;
        case 'node':
          navigate(entry.action.node, null);
          return;
        case 'register-node':
          setDialog({ kind: 'register-node', name: '', url: '' });
          return;
        case 'register-workspace':
          setDialog({ kind: 'register-workspace', host: 'local', path: '' });
          return;
        case 'cancel-run':
          runCommand(api.cancelRun(entry.action.node, entry.action.sessionId));
          return;
        default: {
          const unhandled: never = entry.action;
          throw new Error(`未处理的命令面板动作: ${JSON.stringify(unhandled)}`);
        }
      }
    },
    [navigate, runCommand, setDialog],
  );

  const cycleSession = useCallback(
    (delta: number) => {
      if (selectedNode === null || summary === null) {
        return;
      }
      const ordered = orderedSessions(summary.workspaces);
      if (ordered.length === 0) {
        return;
      }
      const current = ordered.findIndex((entry) => entry.id === selection.sessionId);
      const next =
        current === -1
          ? delta > 0
            ? 0
            : ordered.length - 1
          : (current + delta + ordered.length) % ordered.length;
      const target = ordered[next];
      if (target !== undefined) {
        navigate(selectedNode, target.id);
      }
    },
    [navigate, selectedNode, selection.sessionId, summary],
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
        case 'select-node': {
          const target = state.hub.nodes[command.index];
          if (target !== undefined) {
            event.preventDefault();
            navigate(target.name, null);
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
    navigate,
    paletteOpen,
    state.hub.nodes,
  ]);

  const notice = unavailableNotice(selectedNode, record);
  const running = useMemo(
    () => runningEntries(state.hub.nodes, state.nodes),
    [state.hub.nodes, state.nodes],
  );

  return (
    <>
      <AppShell
        sidebar={
          <Sidebar
            nodes={state.hub.nodes}
            selectedNode={selectedNode}
            onSelectNode={selectNode}
            onRegisterNode={() => { commands.setDialog({ kind: 'register-node', name: '', url: '' }); }}
            onRenameNode={(name) => { commands.setDialog({ kind: 'rename-node', name, next: name }); }}
            onRemoveNode={(name) => { commands.setDialog({ kind: 'remove-node', name }); }}
            notice={notice}
            commandsDisabled={!nodeReady}
            running={running}
            onSelectRunning={(node, id) => { navigate(node, id); }}
            onCancelRunning={(node, id) => { runCommand(api.cancelRun(node, id)); }}
            modelName={summary?.modelName ?? ''}
            workspaces={summary?.workspaces ?? []}
            selectedSessionId={selection.sessionId}
            editing={commands.editing}
            onSelectSession={selectSession}
            onRenameRequest={commands.requestRename}
            onRenameSubmit={commands.renameSession}
            onDeleteRequest={commands.requestDelete}
            onDeleteConfirm={commands.deleteSession}
            onEditCancel={commands.cancelEditing}
            onCreateSession={commands.createSession}
            onAddWorkspace={() => { commands.setDialog({ kind: 'register-workspace', host: 'local', path: '' }); }}
            onRemoveWorkspace={(host, path) => { commands.setDialog({ kind: 'remove-workspace', host, path }); }}
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
          node={selectedNode}
          record={record}
          view={view}
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
          onRegisterWorkspace={() => { commands.setDialog({ kind: 'register-workspace', host: 'local', path: '' }); }}
        />
      </AppShell>
      {commands.dialog !== null ? (
        <Dialog
          title={dialogTitle(commands.dialog)}
          confirmLabel={dialogConfirmLabel(commands.dialog)}
          confirmDisabled={dialogConfirmDisabled(commands.dialog)}
          danger={commands.dialog.kind === 'remove-node'}
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
