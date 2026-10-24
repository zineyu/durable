import { useCallback, useEffect, useMemo, useState } from 'react';
import type { ReactElement, ReactNode } from 'react';
import { api } from './api/client';
import type { ApiResult } from './api/client';
import type { NodeName } from './api/generated/NodeName';
import type { SubmitMode } from './api/generated/SubmitMode';
import { AppShell } from './components/AppShell';
import { CommandPalette } from './components/CommandPalette';
import { Dialog } from './components/Dialog';
import {
  dialogConfirmDisabled,
  dialogConfirmLabel,
  dialogTitle,
  isNodeDialog,
  renderDialogBody,
} from './components/dialog-body';
import type { DialogState } from './components/dialog-body';
import { EmptyState } from './components/EmptyState';
import { ErrorBanner } from './components/ErrorBanner';
import { SessionPane } from './components/SessionPane';
import { ShortcutHelp } from './components/ShortcutHelp';
import { Sidebar } from './components/Sidebar';
import type { SessionEditing } from './components/Sidebar';
import { buildPalette, filterPalette, stepPaletteIndex } from './view/command-palette';
import type { PaletteEntry } from './view/command-palette';
import { emptyDraft, readDraft, readLastSession, writeDraft, writeLastSession } from './view/drafts';
import type { Draft } from './view/drafts';
import { runningEntries } from './view/activity';
import { toSessionId } from './view/ids';
import type { SessionId, WorkspacePath } from './view/ids';
import { nodeStatusLabel, unavailableNotice } from './view/labels';
import { sessionById } from './view/node-state';
import { orderedSessions, sidebarWorkspaces } from './view/sidebar';
import { readLocationFromHash, writeLocationToHash } from './view/session-hash';
import { isEditableTarget, resolveShortcut } from './view/shortcuts';
import type { Selection } from './view/use-client-state';
import { useDocumentTitle } from './view/use-document-title';
import { useClientState } from './view/use-client-state';

const storage = window.localStorage;

function initialSelection(): Selection {
  const location = readLocationFromHash(window.location.hash);
  return { node: location?.node ?? null, sessionId: location?.sessionId ?? null };
}

function initialDraft(): Draft {
  const location = readLocationFromHash(window.location.hash);
  return readDraft(storage, location?.node ?? null, location?.sessionId ?? null);
}

export function App(): ReactElement {
  const [selection, setSelection] = useState<Selection>(initialSelection);
  const { state, dispatch, report, runCommand } = useClientState(selection);
  const [editing, setEditing] = useState<SessionEditing | null>(null);
  const [dialog, setDialog] = useState<DialogState | null>(null);
  const [draft, setDraft] = useState<Draft>(initialDraft);
  const [sendMode, setSendMode] = useState<SubmitMode>('queue');
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
  const session =
    shard === undefined || selection.sessionId === null
      ? undefined
      : sessionById(shard, selection.sessionId);
  const selectedSessionId = session?.id ?? null;
  const selectedUpdatedAt = session?.updatedAt ?? 0;
  const nodeReady = record?.status === 'ok';

  const navigate = useCallback((node: NodeName, sessionId: SessionId | null) => {
    window.location.hash = writeLocationToHash(node, sessionId);
  }, []);

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
      const next = readLocationFromHash(window.location.hash);
      setSelection({ node: next?.node ?? null, sessionId: next?.sessionId ?? null });
      setDraft(readDraft(storage, next?.node ?? null, next?.sessionId ?? null));
      setDialog(null);
      setEditing(null);
      setPaletteOpen(false);
    };
    window.addEventListener('hashchange', apply);
    return () => { window.removeEventListener('hashchange', apply); };
  }, []);

  useEffect(() => {
    if (selectedNode === null || selection.sessionId !== null) {
      return;
    }
    const current = state.hub.nodes.find((entry) => entry.name === selectedNode);
    const currentShard = state.nodes[selectedNode];
    if (current?.status !== 'ok' || currentShard === undefined) {
      return;
    }
    const last = readLastSession(storage, selectedNode);
    if (last === null || !currentShard.sessions.some((entry) => entry.id === last)) {
      return;
    }
    window.location.hash = writeLocationToHash(selectedNode, last);
  }, [selectedNode, selection.sessionId, state.hub.nodes, state.nodes]);

  useEffect(() => {
    if (selectedNode !== null && selection.sessionId !== null) {
      writeLastSession(storage, selectedNode, selection.sessionId);
    }
  }, [selectedNode, selection.sessionId]);

  useEffect(() => {
    if (selectedNode === null || selectedSessionId === null) {
      return;
    }
    dispatch({ type: 'seen', node: selectedNode, sessionId: selectedSessionId, at: selectedUpdatedAt });
  }, [dispatch, selectedNode, selectedSessionId, selectedUpdatedAt]);

  useDocumentTitle(selectedNode, session, shard);

  const changeDraft = useCallback(
    (text: string, pastes: Record<string, string>) => {
      const next: Draft = { text, pastes };
      setDraft(next);
      writeDraft(storage, selectedNode, selection.sessionId, next);
    },
    [selectedNode, selection.sessionId],
  );

  const handleCreateSession = useCallback(
    (workspace: WorkspacePath) => {
      if (selectedNode === null) {
        return;
      }
      const node = selectedNode;
      void (async () => {
        const outcome = await api.createSession(node, workspace, null);
        if (!outcome.ok) {
          report(outcome.error);
          return;
        }
        dispatch({ type: 'session', node, session: outcome.value });
        navigate(node, toSessionId(outcome.value.id));
      })();
    },
    [dispatch, navigate, report, selectedNode],
  );

  const handleRenameSubmit = useCallback(
    (id: SessionId, title: string) => {
      if (selectedNode === null) {
        return;
      }
      const node = selectedNode;
      void (async () => {
        const outcome = await api.renameSession(node, id, title);
        if (!outcome.ok) {
          report(outcome.error);
          return;
        }
        setEditing(null);
      })();
    },
    [report, selectedNode],
  );

  const handleDeleteConfirm = useCallback(
    (id: SessionId) => {
      if (selectedNode === null) {
        return;
      }
      const node = selectedNode;
      void (async () => {
        const outcome = await api.deleteSession(node, id);
        if (!outcome.ok) {
          report(outcome.error);
          return;
        }
        setEditing(null);
        if (id === selection.sessionId) {
          navigate(node, null);
        }
      })();
    },
    [navigate, report, selectedNode, selection.sessionId],
  );

  const handleSend = useCallback(
    (text: string) => {
      if (selectedNode === null || session === undefined) {
        return;
      }
      const trimmed = text.trim();
      if (trimmed.length === 0) {
        return;
      }
      const node = selectedNode;
      const id = session.id;
      const mode = session.busy ? sendMode : 'queue';
      void (async () => {
        const outcome = await api.submitMessage(node, id, trimmed, mode);
        if (!outcome.ok) {
          report(outcome.error);
          return;
        }
        setDraft(emptyDraft);
        writeDraft(storage, node, toSessionId(id), emptyDraft);
      })();
    },
    [report, selectedNode, sendMode, session],
  );

  const handleCancelRun = useCallback(() => {
    if (selectedNode === null || session?.busy !== true) {
      return;
    }
    runCommand(api.cancelRun(selectedNode, session.id));
  }, [runCommand, selectedNode, session]);

  const paletteEntries = useMemo(
    () =>
      filterPalette(
        buildPalette({
          nodes: state.hub.nodes,
          shards: state.nodes,
          selectedNode,
          selectedSessionId: selection.sessionId,
        }),
        paletteQuery,
      ),
    [paletteQuery, selectedNode, selection.sessionId, state.hub.nodes, state.nodes],
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
          setDialog({ kind: 'register-workspace', path: '' });
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
    [navigate, runCommand],
  );

  const cycleSession = useCallback(
    (delta: number) => {
      if (selectedNode === null || shard === undefined) {
        return;
      }
      const ordered = orderedSessions(shard);
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
    [navigate, selectedNode, selection.sessionId, shard],
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
          setDialog(null);
          return;
        }
        handleCancelRun();
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
    cycleSession,
    dialog,
    handleCancelRun,
    helpOpen,
    navigate,
    paletteOpen,
    state.hub.nodes,
  ]);

  const handleDialogConfirm = useCallback(() => {
    if (dialog === null) {
      return;
    }
    const current = dialog;
    const close = () => { setDialog(null); };

    if (isNodeDialog(current)) {
      void (async () => {
        switch (current.kind) {
          case 'register-node': {
            const outcome = await api.registerNode(current.name.trim(), current.url.trim());
            if (!outcome.ok) {
              report(outcome.error);
              return;
            }
            close();
            navigate(outcome.value.name, null);
            return;
          }
          case 'rename-node': {
            const outcome = await api.renameNode(current.name, current.next.trim());
            if (!outcome.ok) {
              report(outcome.error);
              return;
            }
            close();
            if (selectedNode === current.name) {
              navigate(outcome.value.name, selection.sessionId);
            }
            return;
          }
          case 'remove-node': {
            const outcome = await api.removeNode(current.name);
            if (!outcome.ok) {
              report(outcome.error);
              return;
            }
            close();
            if (selectedNode === current.name) {
              window.location.hash = '';
            }
            return;
          }
          default: {
            const unhandled: never = current;
            throw new Error(`未处理的节点对话框: ${JSON.stringify(unhandled)}`);
          }
        }
      })();
      return;
    }

    if (selectedNode === null) {
      return;
    }
    const node = selectedNode;
    void (async () => {
      let outcome: ApiResult<unknown>;
      switch (current.kind) {
        case 'edit':
          outcome = await api.editUserMessage(node, current.sessionId, current.messageId, current.text);
          break;
        case 'retry':
          outcome = await api.retryAssistantMessage(node, current.sessionId, current.messageId);
          break;
        case 'register-workspace':
          outcome = await api.registerWorkspace(node, current.path);
          break;
        case 'remove-workspace':
          outcome = await api.removeWorkspace(node, current.path);
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
      close();
    })();
  }, [dialog, navigate, report, selectedNode, selection.sessionId]);

  const renderMain = (): ReactNode => {
    if (selectedNode === null) {
      return (
        <EmptyState title="选择一个节点" description="在左侧节点切换器中选择一台机器。" />
      );
    }
    if (record === undefined) {
      return (
        <EmptyState
          title="未知节点"
          description={`节点 ${selectedNode} 不在节点表里。它可能已被删除或改名。`}
        />
      );
    }
    if (record.status !== 'ok') {
      return (
        <EmptyState title="节点不可用" description={record.detail ?? nodeStatusLabel[record.status]} />
      );
    }
    if (shard === undefined || session === undefined) {
      return (
        <EmptyState
          title="选择一个 session"
          description="从左侧选择 session，或者注册新的 workspace 后创建 session。"
          actionLabel="注册 workspace"
          onAction={() => { setDialog({ kind: 'register-workspace', path: '' }); }}
        />
      );
    }
    return (
      <SessionPane
        key={session.id}
        node={record.name}
        record={record}
        shard={shard}
        session={session}
        connection={state.connection}
        draft={draft.text}
        pastes={draft.pastes}
        onDraftChange={changeDraft}
        onSend={handleSend}
        onCancelRun={handleCancelRun}
        sendMode={sendMode}
        onModeChange={setSendMode}
        runCommand={runCommand}
        onEditMessage={(messageId, text) => {
          setDialog({ kind: 'edit', sessionId: session.id, messageId, text });
        }}
        onRetryMessage={(messageId) => {
          setDialog({ kind: 'retry', sessionId: session.id, messageId });
        }}
      />
    );
  };

  const workspaces = shard === undefined ? [] : sidebarWorkspaces(shard);
  const notice = unavailableNotice(selectedNode, record);
  const running = runningEntries(state.hub.nodes, state.nodes);

  return (
    <>
      <AppShell
        sidebar={
          <Sidebar
            nodes={state.hub.nodes}
            selectedNode={selectedNode}
            onSelectNode={selectNode}
            onRegisterNode={() => { setDialog({ kind: 'register-node', name: '', url: '' }); }}
            onRenameNode={(name) => { setDialog({ kind: 'rename-node', name, next: name }); }}
            onRemoveNode={(name) => { setDialog({ kind: 'remove-node', name }); }}
            notice={notice}
            commandsDisabled={!nodeReady}
            running={running}
            onSelectRunning={(node, id) => { navigate(node, id); }}
            onCancelRunning={(node, id) => { runCommand(api.cancelRun(node, id)); }}
            modelName={shard?.modelName ?? ''}
            workspaces={workspaces}
            selectedSessionId={selection.sessionId}
            editing={editing}
            onSelectSession={selectSession}
            onRenameRequest={(id) => { setEditing({ sessionId: id, view: 'renaming' }); }}
            onRenameSubmit={handleRenameSubmit}
            onDeleteRequest={(id) => { setEditing({ sessionId: id, view: 'deleting' }); }}
            onDeleteConfirm={handleDeleteConfirm}
            onEditCancel={() => { setEditing(null); }}
            onCreateSession={handleCreateSession}
            onAddWorkspace={() => { setDialog({ kind: 'register-workspace', path: '' }); }}
            onRemoveWorkspace={(path) => { setDialog({ kind: 'remove-workspace', path }); }}
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
        {renderMain()}
      </AppShell>
      {dialog !== null ? (
        <Dialog
          title={dialogTitle(dialog)}
          confirmLabel={dialogConfirmLabel(dialog)}
          confirmDisabled={dialogConfirmDisabled(dialog)}
          danger={dialog.kind === 'remove-node'}
          onConfirm={handleDialogConfirm}
          onCancel={() => { setDialog(null); }}
        >
          {renderDialogBody(dialog, setDialog)}
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
