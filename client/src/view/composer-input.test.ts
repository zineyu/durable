import { describe, expect, it } from 'vitest';

import {
  expandPastes,
  foldPaste,
  needsPasteFold,
  pasteToken,
  stepHistory,
} from './composer-input';

describe('输入历史', () => {
  const history = ['第一条', '第二条', '第三条'];

  it('从空草稿向上回到最近一条', () => {
    expect(stepHistory(history, -1, -1, '')).toEqual({ cursor: 2, text: '第三条' });
  });

  it('继续向上回溯更早的消息，到顶后停住', () => {
    expect(stepHistory(history, 2, -1, '草稿')).toEqual({ cursor: 1, text: '第二条' });
    expect(stepHistory(history, 0, -1, '草稿')).toEqual({ cursor: 0, text: '第一条' });
  });

  it('向下回到较新的消息，越过最新一条恢复草稿', () => {
    expect(stepHistory(history, 0, 1, '草稿')).toEqual({ cursor: 1, text: '第二条' });
    expect(stepHistory(history, 2, 1, '草稿')).toEqual({ cursor: -1, text: '草稿' });
  });

  it('没有历史时保持草稿', () => {
    expect(stepHistory([], -1, -1, '草稿')).toEqual({ cursor: -1, text: '草稿' });
    expect(stepHistory([], -1, 1, '草稿')).toEqual({ cursor: -1, text: '草稿' });
  });
});

describe('长粘贴折叠', () => {
  it('短文本不折叠', () => {
    expect(needsPasteFold('短文本')).toBe(false);
    expect(foldPaste('短文本', 1)).toBeNull();
  });

  it('超长文本折叠为带序号的占位符', () => {
    const text = 'x'.repeat(401);
    const folded = foldPaste(text, 1);
    expect(folded).toEqual({ token: '[粘贴内容 1：401 字符]', text });
    expect(expanded(folded?.token ?? '', folded?.text ?? '')).toBe(text);
  });

  it('行数过多的文本也会折叠', () => {
    const text = Array.from({ length: 13 }, (_, index) => `第 ${index} 行`).join('\n');
    expect(needsPasteFold(text)).toBe(true);
    expect(foldPaste(text, 2)?.token).toBe(pasteToken(2, text));
  });

  it('发送前把占位符还原成原文', () => {
    const first = 'a'.repeat(500);
    const second = 'b'.repeat(600);
    const value = `前 ${pasteToken(1, first)} 中 ${pasteToken(2, second)} 后`;
    expect(expandPastes(value, { [pasteToken(1, first)]: first, [pasteToken(2, second)]: second })).toBe(
      `前 ${first} 中 ${second} 后`,
    );
  });

  it('未知占位符保持原样', () => {
    const value = '保留 [粘贴内容 9：500 字符]';
    expect(expandPastes(value, {})).toBe(value);
  });
});

function expanded(token: string, text: string): string {
  return expandPastes(token, { [token]: text });
}
