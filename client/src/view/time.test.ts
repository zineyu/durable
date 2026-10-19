import { describe, expect, it } from 'vitest';

import { formatUpdatedAt } from './time';

describe('更新时间格式', () => {
  it('补零到 MM-DD HH:mm', () => {
    const date = new Date(2026, 0, 5, 7, 9);
    expect(formatUpdatedAt(date.getTime())).toBe('01-05 07:09');
  });

  it('同一时间戳稳定输出', () => {
    const milliseconds = new Date(2026, 9, 19, 23, 58).getTime();
    expect(formatUpdatedAt(milliseconds)).toBe(formatUpdatedAt(milliseconds));
  });
});
