import { describe, expect, it } from 'vitest';

import type { ContentPart } from '../api/generated/ContentPart';
import type { MessageBody } from '../api/generated/MessageBody';
import {
  arrive,
  begin,
  cachedParts,
  emptyBodyCache,
  fail,
  forget,
  invalidate,
  needs,
  reload,
  retry,
  retrying,
} from './body-cache';

function text(value: string): ContentPart[] {
  return [{ kind: 'text', text: value, providerMeta: null }];
}

function body(id: number, value: string): MessageBody {
  return { id, parts: text(value) };
}

describe('正文缓存', () => {
  it('请求之后不再进入 needs，返回的正文可读，未返回的 id 记为空正文', () => {
    const begun = begin(emptyBodyCache, [1, 2]);

    expect(needs(begun, [1, 2, 3])).toEqual([3]);
    expect(cachedParts(begun, 1)).toBeNull();

    const arrived = arrive(begun, [1, 2], [body(1, '问')]);

    expect(needs(arrived, [1, 2])).toEqual([]);
    expect(cachedParts(arrived, 1)).toEqual(text('问'));
    expect(cachedParts(arrived, 2)).toEqual([]);
  });

  it('失败后不再进入 needs，等待时长随尝试次数递增，预算用尽后停止重试', () => {
    const failed = fail(begin(emptyBodyCache, [1]), [1]);

    expect(needs(failed, [1])).toEqual([]);
    expect(retrying(failed)).toEqual([{ ids: [1], delayMs: 1000 }]);

    const retried = retry(failed, [1]);

    expect(needs(retried, [1])).toEqual([1]);
    expect(retrying(retried)).toEqual([]);

    const failedAgain = fail(begin(retried, [1]), [1]);

    expect(retrying(failedAgain)).toEqual([{ ids: [1], delayMs: 3000 }]);

    const exhausted = fail(begin(failedAgain, [1]), [1]);

    expect(retrying(exhausted)).toEqual([]);
    expect(needs(exhausted, [1])).toEqual([]);
  });

  it('等待时长不同的消息分成不同的组', () => {
    const first = fail(begin(emptyBodyCache, [1]), [1]);
    const second = fail(begin(first, [2]), [2]);
    const mixed = fail(begin(retry(second, [1]), [1]), [1]);

    expect(retrying(mixed)).toEqual([
      { ids: [2], delayMs: 1000 },
      { ids: [1], delayMs: 3000 },
    ]);
  });

  it('快照重载把在途与失败退回待请求，正文与尝试次数保留', () => {
    const cached = arrive(begin(emptyBodyCache, [2]), [2], [body(2, '答')]);
    const failed = fail(begin(cached, [1]), [1]);
    const reloaded = reload(failed, [1, 2]);

    expect(needs(reloaded, [1, 2])).toEqual([1]);
    expect(cachedParts(reloaded, 2)).toEqual(text('答'));

    const failedAgain = fail(begin(reloaded, [1]), [1]);

    expect(retrying(failedAgain)).toEqual([{ ids: [1], delayMs: 3000 }]);
  });

  it('重载只改给出的 id，其他消息的阶段不动', () => {
    const failed = fail(begin(emptyBodyCache, [1, 2]), [1, 2]);
    const reloaded = reload(failed, [1]);

    expect(needs(reloaded, [1])).toEqual([1]);
    expect(needs(reloaded, [2])).toEqual([]);
  });

  it('不在途的响应被丢弃，迟到的失败不产生失败记录', () => {
    const failed = fail(begin(emptyBodyCache, [1]), [1]);

    expect(arrive(failed, [1], [body(1, '迟')])).toBe(failed);
    expect(fail(failed, [1])).toBe(failed);
    expect(cachedParts(failed, 1)).toBeNull();
  });

  it('失效丢弃正文与尝试次数，遗忘丢弃整条', () => {
    const cached = arrive(begin(emptyBodyCache, [1, 2]), [1, 2], [body(1, '问'), body(2, '答')]);
    const invalidated = invalidate(cached, 1);

    expect(cachedParts(invalidated, 1)).toBeNull();
    expect(needs(invalidated, [1])).toEqual([1]);
    expect(needs(invalidate(cached, 2), [1])).toEqual([]);

    const failed = fail(begin(cached, [3]), [3]);

    expect(needs(forget(failed, [3]), [3])).toEqual([3]);
    expect(retrying(forget(failed, [3]))).toEqual([]);
    expect(forget(emptyBodyCache, [])).toBe(emptyBodyCache);
  });
});
