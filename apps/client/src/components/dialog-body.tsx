import type { ReactNode } from 'react';
import type { HostKind } from '../api/generated/HostKind';
import type { HostName } from '../api/generated/HostName';

export type DialogState =
  | { kind: 'edit'; sessionId: string; messageId: number; text: string }
  | { kind: 'retry'; sessionId: string; messageId: number }
  | { kind: 'register-workspace'; host: string; path: string }
  | { kind: 'remove-workspace'; host: string; path: string }
  | { kind: 'register-host'; name: string; hostKind: HostKind; target: string }
  | { kind: 'rename-host'; name: HostName; next: string }
  | { kind: 'remove-host'; name: HostName };

type HostDialog = Extract<
  DialogState,
  { kind: 'register-host' | 'rename-host' | 'remove-host' }
>;

export function isHostDialog(dialog: DialogState): dialog is HostDialog {
  return (
    dialog.kind === 'register-host' ||
    dialog.kind === 'rename-host' ||
    dialog.kind === 'remove-host'
  );
}

export function dialogConfirmDisabled(dialog: DialogState): boolean {
  switch (dialog.kind) {
    case 'register-workspace':
      return dialog.host.trim().length === 0 || dialog.path.trim().length === 0;
    case 'register-host':
      return (
        dialog.name.trim().length === 0 ||
        (dialog.hostKind === 'ssh' && dialog.target.trim().length === 0)
      );
    case 'rename-host':
      return dialog.next.trim().length === 0;
    case 'edit':
    case 'retry':
    case 'remove-workspace':
    case 'remove-host':
      return false;
  }
}

export function dialogTitle(dialog: DialogState): string {
  switch (dialog.kind) {
    case 'edit':
      return '编辑用户消息';
    case 'retry':
      return '确认重试';
    case 'register-workspace':
      return '注册 workspace';
    case 'remove-workspace':
      return '移除 workspace';
    case 'register-host':
      return '注册 host';
    case 'rename-host':
      return '重命名 host';
    case 'remove-host':
      return '删除 host';
    default: {
      const unhandled: never = dialog;
      throw new Error(`未处理的对话框: ${JSON.stringify(unhandled)}`);
    }
  }
}

export function dialogConfirmLabel(dialog: DialogState): string {
  switch (dialog.kind) {
    case 'edit':
      return '确认编辑';
    case 'retry':
      return '重试';
    case 'register-workspace':
      return '注册';
    case 'remove-workspace':
      return '移除';
    case 'register-host':
      return '注册';
    case 'rename-host':
      return '重命名';
    case 'remove-host':
      return '删除';
    default: {
      const unhandled: never = dialog;
      throw new Error(`未处理的对话框: ${JSON.stringify(unhandled)}`);
    }
  }
}

export function renderDialogBody(
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
          onChange={(event) => { setDialog({ ...dialog, text: event.target.value }); }}
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
        <>
          <input
            value={dialog.host}
            aria-label="host 名"
            placeholder="local"
            onChange={(event) => { setDialog({ ...dialog, host: event.target.value }); }}
          />
          <input
            value={dialog.path}
            aria-label="workspace 路径"
            placeholder="/absolute/path"
            autoFocus
            onChange={(event) => { setDialog({ ...dialog, path: event.target.value }); }}
          />
        </>
      );
    case 'remove-workspace':
      return (
        <p>
          移除 {dialog.host} 上的 {dialog.path}。移除只删除注册记录，不触碰磁盘文件。仍有
          session 时 server 会拒绝并说明原因，此操作不可撤销。
        </p>
      );
    case 'register-host':
      return (
        <>
          <input
            value={dialog.name}
            aria-label="host 名"
            placeholder="studio"
            autoFocus
            onChange={(event) => { setDialog({ ...dialog, name: event.target.value }); }}
          />
          <select
            value={dialog.hostKind}
            aria-label="host 种类"
            onChange={(event) => {
              setDialog({ ...dialog, hostKind: event.target.value === 'ssh' ? 'ssh' : 'local' });
            }}
          >
            <option value="local">本机</option>
            <option value="ssh">SSH</option>
          </select>
          {dialog.hostKind === 'ssh' ? (
            <input
              value={dialog.target}
              aria-label="SSH 目标"
              placeholder="studio"
              onChange={(event) => { setDialog({ ...dialog, target: event.target.value }); }}
            />
          ) : null}
          <p>本机 host 直接拉起 executor 子进程；SSH 目标可以写 `~/.ssh/config` 里的别名。</p>
        </>
      );
    case 'rename-host':
      return (
        <>
          <input
            value={dialog.next}
            aria-label="新 host 名"
            autoFocus
            onChange={(event) => { setDialog({ ...dialog, next: event.target.value }); }}
          />
          <p>改名后旧名立即变成未知 host，指向旧名的书签与命令会失效。</p>
        </>
      );
    case 'remove-host':
      return (
        <p>
          删除只移除 host 登记，不触碰那台机器上的文件与进程。仍有 workspace 时 server
          会拒绝并说明原因，此操作不可撤销。
        </p>
      );
    default: {
      const unhandled: never = dialog;
      throw new Error(`未处理的对话框: ${JSON.stringify(unhandled)}`);
    }
  }
}
