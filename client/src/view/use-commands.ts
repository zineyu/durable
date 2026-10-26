import { useCallback, useState } from 'react';
import type { Dispatch } from 'react';
import { api } from '../api/client';
import type { ApiFailure, ApiResult } from '../api/client';
import type { NodeName } from '../api/generated/NodeName';
import type { SubmitMode } from '../api/generated/SubmitMode';
import { isNodeDialog } from '../components/dialog-body';
import type { DialogState } from '../components/dialog-body';
import type { SessionEditing } from '../components/Sidebar';
import { emptyDraft, readDraft, writeDraft } from './drafts';
import type { Draft } from './drafts';
import { toSessionId } from './ids';
import type { SessionId, WorkspacePath } from './ids';
import type { ClientAction } from './reducer';
import type { SessionView } from './types';
import type { Selection } from './use-client-state';

const storage = window.localStorage;

/// 本模块负责全部写命令的处理器与一次性界面状态：对话框、重命名与删除、草稿、
/// 发送模式与 run 取消。它不持有派生视图，也不装配网络接线。
/// 选中位置的读取与写入在 `App.tsx` 与 `session-hash.ts`。

export type CommandsOptions = {
  selection: Selection;
  view: SessionView | null;
  dispatch: Dispatch<ClientAction>;
  report: (failure: ApiFailure) => void;
  runCommand: (result: Promise<ApiResult<unknown>>) => void;
  navigate: (node: NodeName | null, sessionId: SessionId | null) => void;
};

export type Commands = {
  dialog: DialogState | null;
  editing: SessionEditing | null;
  draft: Draft;
  sendMode: SubmitMode;
  /// 打开、改写或关闭对话框。关闭时传 `null`。
  setDialog: (next: DialogState | null) => void;
  closeDialog: () => void;
  confirmDialog: () => void;
  requestRename: (sessionId: SessionId) => void;
  requestDelete: (sessionId: SessionId) => void;
  cancelEditing: () => void;
  changeDraft: (text: string, pastes: Record<string, string>) => void;
  changeMode: (mode: SubmitMode) => void;
  createSession: (workspace: WorkspacePath) => void;
  renameSession: (id: SessionId, title: string) => void;
  deleteSession: (id: SessionId) => void;
  send: (text: string) => void;
  cancelRun: () => void;
  /// 选中位置变化之后重置一次性状态，并按新位置重读草稿。
  resetForLocation: (selection: Selection) => void;
};

export function useCommands({
  selection,
  view,
  dispatch,
  report,
  runCommand,
  navigate,
}: CommandsOptions): Commands {
  const [dialog, setDialog] = useState<DialogState | null>(null);
  const [editing, setEditing] = useState<SessionEditing | null>(null);
  const [draft, setDraft] = useState<Draft>(() =>
    readDraft(storage, selection.node, selection.sessionId),
  );
  const [sendMode, setSendMode] = useState<SubmitMode>('queue');

  const selectedNode = selection.node;
  const selectedSessionId = selection.sessionId;

  const updateDialog = useCallback((next: DialogState | null) => { setDialog(next); }, []);
  const closeDialog = useCallback(() => { setDialog(null); }, []);
  const cancelEditing = useCallback(() => { setEditing(null); }, []);
  const changeMode = useCallback((mode: SubmitMode) => { setSendMode(mode); }, []);

  const requestRename = useCallback((sessionId: SessionId) => {
    setEditing({ sessionId, view: 'renaming' });
  }, []);

  const requestDelete = useCallback((sessionId: SessionId) => {
    setEditing({ sessionId, view: 'deleting' });
  }, []);

  const changeDraft = useCallback(
    (text: string, pastes: Record<string, string>) => {
      const next: Draft = { text, pastes };
      setDraft(next);
      writeDraft(storage, selectedNode, selectedSessionId, next);
    },
    [selectedNode, selectedSessionId],
  );

  const resetForLocation = useCallback((next: Selection) => {
    setDraft(readDraft(storage, next.node, next.sessionId));
    setDialog(null);
    setEditing(null);
  }, []);

  const createSession = useCallback(
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

  const renameSession = useCallback(
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

  const deleteSession = useCallback(
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
          navigate(node, null);
        }
      })();
    },
    [navigate, report, selectedNode, selectedSessionId],
  );

  const send = useCallback(
    (text: string) => {
      if (selectedNode === null || view === null) {
        return;
      }
      const trimmed = text.trim();
      if (trimmed.length === 0) {
        return;
      }
      const node = selectedNode;
      const id = view.session.id;
      const mode = view.session.busy ? sendMode : 'queue';
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
    [report, selectedNode, sendMode, view],
  );

  const cancelRun = useCallback(() => {
    if (selectedNode === null || view?.session.busy !== true) {
      return;
    }
    runCommand(api.cancelRun(selectedNode, view.session.id));
  }, [runCommand, selectedNode, view]);

  const confirmDialog = useCallback(() => {
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
              navigate(outcome.value.name, selectedSessionId);
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
              navigate(null, null);
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
          outcome = await api.editUserMessage(
            node,
            current.sessionId,
            current.messageId,
            current.text,
          );
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
  }, [dialog, navigate, report, selectedNode, selectedSessionId]);

  return {
    dialog,
    editing,
    draft,
    sendMode,
    setDialog: updateDialog,
    closeDialog,
    confirmDialog,
    requestRename,
    requestDelete,
    cancelEditing,
    changeDraft,
    changeMode,
    createSession,
    renameSession,
    deleteSession,
    send,
    cancelRun,
    resetForLocation,
  };
}
