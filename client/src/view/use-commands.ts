import { useCallback, useState } from 'react';
import type { Dispatch } from 'react';
import { api } from '../api/client';
import type { ApiFailure, ApiResult } from '../api/client';
import type { SubmitMode } from '../api/generated/SubmitMode';
import { isHostDialog } from '../components/dialog-body';
import type { DialogState } from '../components/dialog-body';
import type { SessionEditing } from '../components/Sidebar';
import { emptyDraft, readDraft, writeDraft } from './drafts';
import type { Draft } from './drafts';
import { toSessionId } from './ids';
import type { SessionId, WorkspacePath } from './ids';
import type { ClientAction } from './reducer';
import type { Selection } from './session-hash';
import type { SessionView } from './types';

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
  navigate: (sessionId: Selection) => void;
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
  createSession: (host: string, workspace: WorkspacePath) => void;
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
  const [draft, setDraft] = useState<Draft>(() => readDraft(storage, selection));
  const [sendMode, setSendMode] = useState<SubmitMode>('queue');

  const selectedSessionId = selection;

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
      writeDraft(storage, selectedSessionId, next);
    },
    [selectedSessionId],
  );

  const resetForLocation = useCallback((next: Selection) => {
    setDraft(readDraft(storage, next));
    setDialog(null);
    setEditing(null);
  }, []);

  const createSession = useCallback(
    (host: string, workspace: WorkspacePath) => {
      void (async () => {
        const outcome = await api.createSession(host, workspace, null);
        if (!outcome.ok) {
          report(outcome.error);
          return;
        }
        dispatch({ type: 'session', session: outcome.value });
        navigate(toSessionId(outcome.value.id));
      })();
    },
    [dispatch, navigate, report],
  );

  const renameSession = useCallback(
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

  const deleteSession = useCallback(
    (id: SessionId) => {
      void (async () => {
        const outcome = await api.deleteSession(id);
        if (!outcome.ok) {
          report(outcome.error);
          return;
        }
        setEditing(null);
        if (id === selectedSessionId) {
          navigate(null);
        }
      })();
    },
    [navigate, report, selectedSessionId],
  );

  const send = useCallback(
    (text: string) => {
      if (view === null) {
        return;
      }
      const trimmed = text.trim();
      if (trimmed.length === 0) {
        return;
      }
      const id = view.session.id;
      const mode = view.session.busy ? sendMode : 'queue';
      void (async () => {
        const outcome = await api.submitMessage(id, trimmed, mode);
        if (!outcome.ok) {
          report(outcome.error);
          return;
        }
        setDraft(emptyDraft);
        writeDraft(storage, toSessionId(id), emptyDraft);
      })();
    },
    [report, sendMode, view],
  );

  const cancelRun = useCallback(() => {
    if (view?.session.busy !== true) {
      return;
    }
    runCommand(api.cancelRun(view.session.id));
  }, [runCommand, view]);

  const confirmDialog = useCallback(() => {
    if (dialog === null) {
      return;
    }
    const current = dialog;
    const close = () => { setDialog(null); };

    if (isHostDialog(current)) {
      void (async () => {
        switch (current.kind) {
          case 'register-host': {
            const outcome = await api.registerHost(
              current.name.trim(),
              current.hostKind,
              current.hostKind === 'ssh' ? current.target.trim() : null,
            );
            if (!outcome.ok) {
              report(outcome.error);
              return;
            }
            close();
            return;
          }
          case 'rename-host': {
            const outcome = await api.updateHost(current.name, {
              name: current.next.trim(),
            });
            if (!outcome.ok) {
              report(outcome.error);
              return;
            }
            close();
            return;
          }
          case 'remove-host': {
            const outcome = await api.removeHost(current.name);
            if (!outcome.ok) {
              report(outcome.error);
              return;
            }
            close();
            return;
          }
          default: {
            const unhandled: never = current;
            throw new Error(`未处理的 host 对话框: ${JSON.stringify(unhandled)}`);
          }
        }
      })();
      return;
    }

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
          outcome = await api.registerWorkspace(current.host, current.path);
          break;
        case 'remove-workspace':
          outcome = await api.removeWorkspace(current.host, current.path);
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
  }, [dialog, report]);

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
