import { describe, expect, it } from 'vitest';

import { resolveShortcut, shortcutHelp } from './shortcuts';

function key(
  value: string,
  extra: Partial<{ ctrlKey: boolean; metaKey: boolean; altKey: boolean; shiftKey: boolean; code: string }> = {},
) {
  return {
    key: value,
    code: extra.code ?? '',
    ctrlKey: extra.ctrlKey ?? false,
    metaKey: extra.metaKey ?? false,
    altKey: extra.altKey ?? false,
    shiftKey: extra.shiftKey ?? false,
  };
}

describe('快捷键', () => {
  it('命令面板在输入框聚焦时仍然可用', () => {
    expect(resolveShortcut(key('k', { metaKey: true }), { editable: true })).toEqual({
      type: 'open-palette',
    });
    expect(resolveShortcut(key('k', { ctrlKey: true }), { editable: true })).toEqual({
      type: 'open-palette',
    });
  });

  it('带其它修饰键的 K 不打开面板', () => {
    expect(resolveShortcut(key('k', { ctrlKey: true, altKey: true }), { editable: false })).toBeNull();
    expect(resolveShortcut(key('k'), { editable: false })).toBeNull();
  });

  it('Esc 在输入框聚焦时仍然可用，用于取消运行', () => {
    expect(resolveShortcut(key('Escape'), { editable: true })).toEqual({ type: 'escape' });
  });

  it('帮助键在输入框聚焦时让位给文本输入', () => {
    expect(resolveShortcut(key('?', { shiftKey: true }), { editable: false })).toEqual({
      type: 'toggle-help',
    });
    expect(resolveShortcut(key('?', { shiftKey: true }), { editable: true })).toBeNull();
  });

  it('Alt+方向键切换 session，并在输入框聚焦时让位', () => {
    expect(resolveShortcut(key('ArrowUp', { altKey: true }), { editable: false })).toEqual({
      type: 'cycle-session',
      delta: -1,
    });
    expect(resolveShortcut(key('ArrowDown', { altKey: true }), { editable: false })).toEqual({
      type: 'cycle-session',
      delta: 1,
    });
    expect(resolveShortcut(key('ArrowDown', { altKey: true }), { editable: true })).toBeNull();
  });

  it('Alt+数字按物理键位切换节点', () => {
    expect(
      resolveShortcut(key('¡', { altKey: true, code: 'Digit1' }), { editable: false }),
    ).toEqual({ type: 'select-node', index: 0 });
    expect(
      resolveShortcut(key('9', { altKey: true, code: 'Numpad9' }), { editable: false }),
    ).toEqual({ type: 'select-node', index: 8 });
    expect(
      resolveShortcut(key('0', { altKey: true, code: 'Digit0' }), { editable: false }),
    ).toBeNull();
    expect(
      resolveShortcut(key('1', { altKey: true, code: 'Digit1' }), { editable: true }),
    ).toBeNull();
  });

  it('帮助清单覆盖全部快捷键', () => {
    const keys = shortcutHelp.map((entry) => entry.keys).join(' ');
    for (const fragment of ['K', '?', 'Esc', 'Alt + ↑', 'Alt + 1']) {
      expect(keys).toContain(fragment);
    }
  });
});
