/// 全局快捷键的解析。组件只把键盘事件交给它，按返回的命令执行。
export type ShortcutCommand =
  | { type: 'open-palette' }
  | { type: 'toggle-help' }
  | { type: 'escape' }
  | { type: 'cycle-session'; delta: number }
  | { type: 'select-host'; index: number };

export type ShortcutEvent = {
  key: string;
  code: string;
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
};

/// 输入框聚焦时全部让位给文本输入，`Esc` 与命令面板除外。
export type ShortcutContext = {
  editable: boolean;
};

export type ShortcutHelpEntry = {
  keys: string;
  description: string;
};

export const shortcutHelp: ShortcutHelpEntry[] = [
  { keys: 'Ctrl/⌘ + K', description: '打开命令面板' },
  { keys: '?', description: '查看全部快捷键' },
  { keys: 'Esc', description: '取消当前 session 的运行' },
  { keys: 'Alt + ↑ / ↓', description: '切换上一个或下一个 session' },
  { keys: 'Alt + 1…9', description: '切换到第 n 个 host 最近的 session' },
  { keys: '↑ / ↓', description: '在输入框里回溯已发送的消息' },
];

export function resolveShortcut(
  event: ShortcutEvent,
  context: ShortcutContext,
): ShortcutCommand | null {
  if (isPaletteKey(event)) {
    return { type: 'open-palette' };
  }
  if (event.key === 'Escape') {
    return { type: 'escape' };
  }
  if (context.editable) {
    return null;
  }
  if (event.key === '?') {
    return { type: 'toggle-help' };
  }
  if (event.altKey && !event.ctrlKey && !event.metaKey) {
    if (event.key === 'ArrowUp') {
      return { type: 'cycle-session', delta: -1 };
    }
    if (event.key === 'ArrowDown') {
      return { type: 'cycle-session', delta: 1 };
    }
    const index = hostDigit(event.code);
    if (index !== null) {
      return { type: 'select-host', index };
    }
  }
  return null;
}

/// 目标是输入框、文本域或可编辑元素时，快捷键让位给文本输入。
export function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) {
    return false;
  }
  if (target.isContentEditable) {
    return true;
  }
  return target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.tagName === 'SELECT';
}

function isPaletteKey(event: ShortcutEvent): boolean {
  return (
    (event.ctrlKey || event.metaKey) &&
    !event.altKey &&
    event.key.toLowerCase() === 'k'
  );
}

function hostDigit(code: string): number | null {
  const match = /^(?:Digit|Numpad)([1-9])$/.exec(code);
  if (match === null) {
    return null;
  }
  const digit = match[1];
  return digit === undefined ? null : Number(digit) - 1;
}
