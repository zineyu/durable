import type { ReactNode } from 'react';
import type { NodeName } from '../api/generated/NodeName';

export type DialogState =
  | { kind: 'edit'; sessionId: string; messageId: number; text: string }
  | { kind: 'retry'; sessionId: string; messageId: number }
  | { kind: 'register-workspace'; path: string }
  | { kind: 'remove-workspace'; path: string }
  | { kind: 'register-node'; name: string; url: string }
  | { kind: 'rename-node'; name: NodeName; next: string }
  | { kind: 'remove-node'; name: NodeName };

type NodeDialog = Extract<
  DialogState,
  { kind: 'register-node' | 'rename-node' | 'remove-node' }
>;

export function isNodeDialog(dialog: DialogState): dialog is NodeDialog {
  return (
    dialog.kind === 'register-node' ||
    dialog.kind === 'rename-node' ||
    dialog.kind === 'remove-node'
  );
}

export function dialogConfirmDisabled(dialog: DialogState): boolean {
  switch (dialog.kind) {
    case 'register-workspace':
      return dialog.path.trim().length === 0;
    case 'register-node':
      return dialog.name.trim().length === 0 || dialog.url.trim().length === 0;
    case 'rename-node':
      return dialog.next.trim().length === 0;
    case 'edit':
    case 'retry':
    case 'remove-workspace':
    case 'remove-node':
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
        <input
          value={dialog.path}
          aria-label="workspace 路径"
          placeholder="/absolute/path"
          autoFocus
          onChange={(event) => { setDialog({ ...dialog, path: event.target.value }); }}
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
            onChange={(event) => { setDialog({ ...dialog, name: event.target.value }); }}
          />
          <input
            value={dialog.url}
            aria-label="节点 URL"
            placeholder="http://127.0.0.1:8081"
            onChange={(event) => { setDialog({ ...dialog, url: event.target.value }); }}
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
            onChange={(event) => { setDialog({ ...dialog, next: event.target.value }); }}
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
