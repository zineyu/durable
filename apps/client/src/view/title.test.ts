import { describe, expect, it } from 'vitest';

import { documentTitle } from './title';

describe('页面标题', () => {
  it('没有选中 session 时只显示产品名', () => {
    expect(documentTitle(null)).toBe('durable');
  });

  it('运行中与排队中带状态前缀', () => {
    expect(
      documentTitle({
        host: 'macbook-air-01',
        title: '类型生成一致性检查',
        state: 'running',
        lastRun: 'running',
      }),
    ).toBe('执行中 · 类型生成一致性检查 · macbook-air-01');
    expect(
      documentTitle({
        host: 'macbook-air-01',
        title: '类型生成一致性检查',
        state: 'queued',
        lastRun: 'succeeded',
      }),
    ).toBe('排队中 · 类型生成一致性检查 · macbook-air-01');
  });

  it('空闲时按最近一次 run 的结局加前缀', () => {
    expect(
      documentTitle({
        host: 'studio',
        title: 'SQLite 迁移与恢复',
        state: 'idle',
        lastRun: 'failed',
      }),
    ).toBe('失败 · SQLite 迁移与恢复 · studio');
    expect(
      documentTitle({
        host: 'studio',
        title: 'SQLite 迁移与恢复',
        state: 'idle',
        lastRun: 'cancelled',
      }),
    ).toBe('已取消 · SQLite 迁移与恢复 · studio');
    expect(
      documentTitle({
        host: 'studio',
        title: 'SQLite 迁移与恢复',
        state: 'idle',
        lastRun: 'succeeded',
      }),
    ).toBe('SQLite 迁移与恢复 · studio');
  });
});
