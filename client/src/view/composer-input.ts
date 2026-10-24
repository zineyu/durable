/// 输入框的历史回溯与长粘贴折叠。两者都只在客户端完成。
export type HistoryStep = {
  /// `-1` 表示正在编辑尚未发送的草稿。
  cursor: number;
  text: string;
};

const PASTE_FOLD_CHARACTERS = 400;
const PASTE_FOLD_LINES = 12;

/// 在已发送的历史与草稿之间移动。`delta` 为 `-1` 向更早，`1` 向更新。
export function stepHistory(
  history: string[],
  cursor: number,
  delta: number,
  draft: string,
): HistoryStep {
  if (history.length === 0) {
    return { cursor: -1, text: draft };
  }
  if (delta < 0) {
    const next = cursor < 0 ? history.length - 1 : Math.max(0, cursor - 1);
    return { cursor: next, text: history[next] ?? draft };
  }
  if (cursor < 0) {
    return { cursor: -1, text: draft };
  }
  const next = cursor + 1;
  if (next >= history.length) {
    return { cursor: -1, text: draft };
  }
  return { cursor: next, text: history[next] ?? draft };
}

export function needsPasteFold(text: string): boolean {
  return text.length > PASTE_FOLD_CHARACTERS || text.split('\n').length > PASTE_FOLD_LINES;
}

export function pasteToken(ordinal: number, text: string): string {
  return `[粘贴内容 ${ordinal}：${text.length} 字符]`;
}

/// 超长粘贴折叠为一个占位符。短文本不需要折叠时返回 `null`。
export function foldPaste(text: string, ordinal: number): { token: string; text: string } | null {
  if (!needsPasteFold(text)) {
    return null;
  }
  return { token: pasteToken(ordinal, text), text };
}

/// 发送前把占位符替换回原文。没有记录的占位符保持原样。
export function expandPastes(value: string, pastes: Record<string, string>): string {
  let expanded = value;
  for (const [token, text] of Object.entries(pastes)) {
    expanded = expanded.split(token).join(text);
  }
  return expanded;
}
