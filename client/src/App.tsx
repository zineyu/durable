import { useCallback, useEffect, useReducer, useRef, useState } from 'react';
import type { ReactNode } from 'react';
import { api } from './api/client';
import type { ApiFailure, ApiResult } from './api/client';
import { openEventStream } from './api/events';
import type { EventStream } from './api/events';
import type { HubBootstrap } from './api/generated/HubBootstrap';
import type { MessageNode } from './api/generated/MessageNode';
import type { NodeName } from './api/generated/NodeName';
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
  messageView,
  sessionById,
  sessionQueue,
  sidebarWorkspaces,
  snapshotCursor,
  transcriptNodes,
} from './view/node-state';
import { initialClientState, reduce } from './view/reducer';
import type { ClientState } from './view/reducer';
import { readLocationFromHash, writeLocationToHash } from './view/session-hash';
import { nodeStatusLabel } from './view/labels';
import type { BranchControl, NodeRecord, SessionState } from './view/types';

type DialogState =
  | { kind: 'edit'; sessionId: string; messageId: number; text: string }
  | { kind: 'retry'; sessionId: string; messageId: number }
  | { kind: 'register-workspace'; path: string }
  | { kind: 'remove-workspace'; path: string }
  | { kind: 'register-node'; name: string; url: string }
  | { kind: 'rename-node'; name: NodeName; next: string }
  | { kind: 'remove-node'; name: NodeName };

const RECONNECT_DELAY_MS = 1000;

function sessionStateOf(busy: boolean, queueLength: number): SessionState {
  if (busy) {
    return 'running';
  }
  return queueLength > 0 ? 'queued' : 'idle';
}

function initialStateFromHash(): ClientState {
  const location = readLocationFromHash(window.location.hash);
  return reduce(initialClientState, { type: 'select-node', node: location?.node ?? null });
}

/// 侧栏顶部切换器下方的说明。节点可用时没有说明。
function unavailableNotice(
  selected: NodeName | null,
  record: NodeRecord | undefined,
): string | null {
  if (selected === null) {
    return null;
  }
  if (record === undefined) {
    return '未知节点，节点表里没有这个名字';
  }
  if (record.status === 'ok') {
    return null;
  }
  return record.detail ?? nodeStatusLabel[record.status];
}

export function App() {
  const [state, dispatch] = useReducer(reduce, undefined, initialStateFromHash);
  const [selectedSessionId, setSelectedSessionId] = useState<SessionId | null>(
    () => readLocationFromHash(window.location.hash)?.sessionId ?? null,
  );
  const [editing, setEditing] = useState<SessionEditing | null>(null);
  const [dialog, setDialog] = useState<DialogState | null>(null);
  const [draft, setDraft] = useState('');

  const selectedNode = state.hub.selected;
  const record =
    selectedNode === null
      ? undefined
      : state.hub.nodes.find((entry) => entry.name === selectedNode);
  const shard = selectedNode === null ? undefined : state.nodes[selectedNode];
  const session =
    shard === undefined || selectedSessionId === null
      ? undefined
      : sessionById(shard, selectedSessionId);
  const nodeReady = record?.status === 'ok';

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

  const streamRef = useRef<EventStream | null>(null);
  const cursorRef = useRef<number | null>(null);
  const selectionRef = useRef<{ node: NodeName | null; sessionId: SessionId | null }>({
    node: selectedNode,
    sessionId: selectedSessionId,
  });

  useEffect(() => {
    cursorRef.current = state.hub.cursor;
  }, [state.hub.cursor]);

  useEffect(() => {
    selectionRef.current = { node: selectedNode, sessionId: selectedSessionId };
  }, [selectedNode, selectedSessionId]);

  useEffect(() => {
    const apply = () => {
      const next = readLocationFromHash(window.location.hash);
      dispatch({ type: 'select-node', node: next?.node ?? null });
      setSelectedSessionId(next?.sessionId ?? null);
    };
    window.addEventListener('hashchange', apply);
    return () => window.removeEventListener('hashchange', apply);
  }, []);

  useEffect(() => {
    setDialog(null);
    setEditing(null);
  }, [selectedNode]);

  const missingKey = state.hub.nodes
    .filter((entry) => entry.status === 'ok' && state.nodes[entry.name] === undefined)
    .map((entry) => entry.name)
    .join(',');

  useEffect(() => {
    if (missingKey === '') {
      return;
    }
    const missing = missingKey.split(',');
    let active = true;
    void (async () => {
      for (const name of missing) {
        const outcome = await api.nodeBootstrap(name);
        if (!active) {
          return;
        }
        if (!outcome.ok) {
          report(outcome.error);
          continue;
        }
        dispatch({ type: 'node-bootstrap', node: name, bootstrap: outcome.value });
      }
    })();
    return () => {
      active = false;
    };
  }, [missingKey, report]);

  useEffect(() => {
    let active = true;
    let retry: ReturnType<typeof setTimeout> | null = null;

    const delay = (milliseconds: number) =>
      new Promise<void>((resolve) => {
        retry = setTimeout(resolve, milliseconds);
      });

    const refresh = async (): Promise<number | null> => {
      const outcome = await api.bootstrap();
      if (!active) {
        return null;
      }
      if (!outcome.ok) {
        report(outcome.error);
        return null;
      }
      const cursor = snapshotCursor(cursorRef.current, outcome.value.maxHubSeq);
      dispatch({ type: 'bootstrap', bootstrap: outcome.value });
      const selection = selectionRef.current;
      if (selection.node !== null && selection.sessionId !== null) {
        const tree = await api.sessionTree(selection.node, selection.sessionId);
        if (!active) {
          return null;
        }
        if (!tree.ok) {
          report(tree.error);
          return null;
        }
        dispatch({ type: 'tree', node: selection.node, tree: tree.value });
      }
      return cursor;
    };

    const reconnect = async () => {
      for (;;) {
        const cursor = await refresh();
        if (!active) {
          return;
        }
        if (cursor !== null) {
          streamRef.current?.resume(cursor);
          return;
        }
        await delay(RECONNECT_DELAY_MS);
        if (!active) {
          return;
        }
      }
    };

    const connect = (bootstrap: HubBootstrap) => {
      const cursor = snapshotCursor(cursorRef.current, bootstrap.maxHubSeq);
      dispatch({ type: 'bootstrap', bootstrap });
      streamRef.current = openEventStream(cursor, {
        onEvent: (event) => dispatch({ type: 'stream', event }),
        onState: (connection) => dispatch({ type: 'connection', connection }),
        onDisconnect: () => {
          void reconnect();
        },
      });
    };

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
      connect(outcome.value);
    })();

    return () => {
      active = false;
      if (retry !== null) {
        clearTimeout(retry);
      }
      streamRef.current?.close();
      streamRef.current = null;
    };
  }, [report]);

  const sessionExists = session !== undefined;

  useEffect(() => {
    if (selectedNode === null || selectedSessionId === null || !sessionExists) {
      return;
    }
    const node = selectedNode;
    const id = selectedSessionId;
    let active = true;
    void (async () => {
      const outcome = await api.sessionTree(node, id);
      if (!active) {
        return;
      }
      if (!outcome.ok) {
        report(outcome.error);
        return;
      }
      dispatch({ type: 'tree', node, tree: outcome.value });
    })();
    return () => {
      active = false;
    };
  }, [selectedNode, selectedSessionId, sessionExists, report]);

  const selectNode = useCallback((node: NodeName) => {
    window.location.hash = writeLocationToHash(node, null);
  }, []);

  const selectSession = useCallback(
    (id: SessionId) => {
      if (selectedNode === null) {
        return;
      }
      window.location.hash = writeLocationToHash(selectedNode, id);
    },
    [selectedNode],
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
        window.location.hash = writeLocationToHash(node, toSessionId(outcome.value.id));
      })();
    },
    [selectedNode, report],
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
    [selectedNode, report],
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
        if (id === selectedSessionId) {
          window.location.hash = writeLocationToHash(node, null);
        }
      })();
    },
    [selectedNode, selectedSessionId, report],
  );

  const handleSend = useCallback(() => {
    if (selectedNode === null || session === undefined) {
      return;
    }
    const text = draft.trim();
    if (text.length === 0) {
      return;
    }
    const node = selectedNode;
    const id = session.id;
    void (async () => {
      const outcome = await api.submitMessage(node, id, text);
      if (!outcome.ok) {
        report(outcome.error);
        return;
      }
      setDraft('');
    })();
  }, [draft, session, selectedNode, report]);

  const handleCancelRun = useCallback(() => {
    if (selectedNode === null || session === undefined) {
      return;
    }
    runCommand(api.cancelRun(selectedNode, session.id));
  }, [session, selectedNode, runCommand]);

  const handleDialogConfirm = useCallback(() => {
    if (dialog === null) {
      return;
    }
    const current = dialog;
    const close = () => setDialog(null);

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
            window.location.hash = writeLocationToHash(outcome.value.name, null);
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
              window.location.hash = writeLocationToHash(outcome.value.name, selectedSessionId);
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
  }, [dialog, selectedNode, selectedSessionId, report]);

  const renderMain = (): ReactNode => {
    if (selectedNode === null) {
      return (
        <EmptyState
          title="选择一个节点"
          description="在左侧节点切换器中选择一台机器。"
        />
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
        <EmptyState
          title="节点不可用"
          description={record.detail ?? nodeStatusLabel[record.status]}
        />
      );
    }
    if (shard === undefined || session === undefined) {
      return (
        <EmptyState
          title="选择一个 session"
          description="从左侧选择 session，或者注册新的 workspace 后创建 session。"
          actionLabel="注册 workspace"
          onAction={() => setDialog({ kind: 'register-workspace', path: '' })}
        />
      );
    }

    const current = session;
    const idle = !current.busy && current.queueLength === 0;
    const nodeName = record.name;

    const controls = (message: MessageNode): { branch?: BranchControl } => {
      const position = branchPosition(shard, current.id, message);
      if (position === null) {
        return {};
      }
      const control: BranchControl = { index: position.index, total: position.total };
      if (!idle) {
        return { branch: control };
      }
      const previousId = position.previousId;
      const nextId = position.nextId;
      if (previousId !== null) {
        control.onPrev = () =>
          runCommand(api.switchActiveLeaf(nodeName, current.id, previousId));
      }
      if (nextId !== null) {
        control.onNext = () => runCommand(api.switchActiveLeaf(nodeName, current.id, nextId));
      }
      return { branch: control };
    };

    const entries = transcriptNodes(shard, current.id).map((message) => {
      if (message.role === 'user') {
        return (
          <UserMessage
            key={message.id}
            text={message.text}
            onEdit={() =>
              setDialog({
                kind: 'edit',
                sessionId: current.id,
                messageId: message.id,
                text: message.text,
              })
            }
            editDisabled={!idle}
            {...controls(message)}
          />
        );
      }
      const view = messageView(shard, current.id, message);
      return (
        <AssistantMessage
          key={message.id}
          state={view.state}
          {...(view.note === null ? {} : { note: view.note })}
          onRetry={() =>
            setDialog({ kind: 'retry', sessionId: current.id, messageId: message.id })
          }
          retryDisabled={!idle}
          {...controls(message)}
        >
          <AssistantContent text={message.text} toolCalls={message.toolCalls} />
        </AssistantMessage>
      );
    });

    const queue = sessionQueue(shard, current.id).map((item) => ({
      id: toQueueItemId(String(item.id)),
      text: item.text,
    }));

    return (
      <>
        <MainHeader
          node={nodeName}
          nodeStatus={record.status}
          nodeDetail={record.detail}
          title={current.title}
          workspacePath={toWorkspacePath(current.workspace)}
          state={sessionStateOf(current.busy, current.queueLength)}
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
          onRemove={(viewId) =>
            runCommand(api.removeQueueItem(nodeName, current.id, Number(viewId)))
          }
        />
        <Composer
          value={draft}
          onChange={setDraft}
          onSend={handleSend}
          {...(current.busy ? { onCancel: handleCancelRun } : {})}
        />
      </>
    );
  };

  const workspaces = shard === undefined ? [] : sidebarWorkspaces(shard);
  const notice = unavailableNotice(selectedNode, record);

  return (
    <>
      <AppShell
        sidebar={
          <Sidebar
            nodes={state.hub.nodes}
            selectedNode={selectedNode}
            onSelectNode={selectNode}
            onRegisterNode={() => setDialog({ kind: 'register-node', name: '', url: '' })}
            onRenameNode={(name) => setDialog({ kind: 'rename-node', name, next: name })}
            onRemoveNode={(name) => setDialog({ kind: 'remove-node', name })}
            notice={notice}
            commandsDisabled={!nodeReady}
            modelName={shard?.modelName ?? ''}
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
            onRemoveWorkspace={(path) => setDialog({ kind: 'remove-workspace', path })}
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
        {renderMain()}
      </AppShell>
      {dialog !== null ? (
        <Dialog
          title={dialogTitle(dialog)}
          confirmLabel={dialogConfirmLabel(dialog)}
          confirmDisabled={dialogConfirmDisabled(dialog)}
          danger={dialog.kind === 'remove-node'}
          onConfirm={handleDialogConfirm}
          onCancel={() => setDialog(null)}
        >
          {renderDialogBody(dialog, setDialog)}
        </Dialog>
      ) : null}
    </>
  );
}

type NodeDialog = Extract<
  DialogState,
  { kind: 'register-node' | 'rename-node' | 'remove-node' }
>;

function isNodeDialog(dialog: DialogState): dialog is NodeDialog {
  return (
    dialog.kind === 'register-node' ||
    dialog.kind === 'rename-node' ||
    dialog.kind === 'remove-node'
  );
}

function dialogConfirmDisabled(dialog: DialogState): boolean {
  switch (dialog.kind) {
    case 'register-workspace':
      return dialog.path.trim().length === 0;
    case 'register-node':
      return dialog.name.trim().length === 0 || dialog.url.trim().length === 0;
    case 'rename-node':
      return dialog.next.trim().length === 0;
    default:
      return false;
  }
}

function dialogTitle(dialog: DialogState): string {
  switch (dialog.kind) {
    case 'edit':
      return '编辑用户消息';
    case 'retry':
      return '确认重试';
    case 'register-workspace':
      return '注册 workspace';
    case 'remove-workspace':
      return '移除 workspace';
    case 'register-node':
      return '注册节点';
    case 'rename-node':
      return '重命名节点';
    case 'remove-node':
      return '删除节点';
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
    case 'remove-workspace':
      return '移除';
    case 'register-node':
      return '注册';
    case 'rename-node':
      return '重命名';
    case 'remove-node':
      return '删除';
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
    case 'remove-workspace':
      return (
        <p>
          移除只删除注册记录，不触碰磁盘文件。仍有 session 时 server 会拒绝并说明原因，此操作不可撤销。
        </p>
      );
    case 'register-node':
      return (
        <>
          <input
            value={dialog.name}
            aria-label="节点名"
            placeholder="macbook-air-02"
            autoFocus
            onChange={(event) => setDialog({ ...dialog, name: event.target.value })}
          />
          <input
            value={dialog.url}
            aria-label="节点 URL"
            placeholder="http://127.0.0.1:8081"
            onChange={(event) => setDialog({ ...dialog, url: event.target.value })}
          />
        </>
      );
    case 'rename-node':
      return (
        <>
          <input
            value={dialog.next}
            aria-label="新节点名"
            autoFocus
            onChange={(event) => setDialog({ ...dialog, next: event.target.value })}
          />
          <p>改名后旧链接失效，指向旧名的页面会显示「未知节点」。</p>
        </>
      );
    case 'remove-node':
      return (
        <p>
          删除只移除节点表登记，不触碰那台机器上的 agent、SQLite 与在跑的 run。此操作不可撤销。
        </p>
      );
    default: {
      const unhandled: never = dialog;
      throw new Error(`未处理的对话框: ${JSON.stringify(unhandled)}`);
    }
  }
}
