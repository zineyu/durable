/// 本模块负责正文缓存：一条消息的块正文、拉取阶段与尝试预算。
/// 呈现层经 `derived.ts` 读它，时间效果在 `use-message-bodies.ts`，未定型占位在 `placeholders.ts`。
import type { ContentPart } from '../api/generated/ContentPart';
import type { MessageBody } from '../api/generated/MessageBody';

/// 失败后的重试等待。档位数加一即单条消息的尝试上限。
const RETRY_DELAYS_MS = [1000, 3000];

/// 一条消息的拉取阶段。`ready` 表示该拉取正文：尚未发出请求，或失败之后已被放行重试。
type Entry =
  | { phase: 'ready'; attempts: number }
  | { phase: 'pending'; attempts: number }
  | { phase: 'cached'; parts: ContentPart[] }
  | { phase: 'failed'; attempts: number };

/// 正文缓存。条目以消息 id 为键，没有条目即未曾请求。
export type BodyCache = { entries: Record<number, Entry> };

export const emptyBodyCache: BodyCache = { entries: {} };

/// 还需要正文的 id：未曾请求，或失败之后已被放行重试。
export function needs(cache: BodyCache, ids: number[]): number[] {
  return ids.filter((id) => {
    const entry = cache.entries[id];
    return entry === undefined || entry.phase === 'ready';
  });
}

/// 已缓存的正文。未缓存时为 `null`。
export function cachedParts(cache: BodyCache, id: number): ContentPart[] | null {
  const entry = cache.entries[id];
  return entry?.phase === 'cached' ? entry.parts : null;
}

/// 该重试的消息，按等待时长分组。预算用尽的消息不在其中。
export function retrying(cache: BodyCache): { ids: number[]; delayMs: number }[] {
  const groups = new Map<number, number[]>();
  for (const [key, entry] of Object.entries(cache.entries)) {
    if (entry.phase !== 'failed') {
      continue;
    }
    const delayMs = RETRY_DELAYS_MS[entry.attempts - 1];
    if (delayMs === undefined) {
      continue;
    }
    const group = groups.get(delayMs);
    if (group === undefined) {
      groups.set(delayMs, [Number(key)]);
    } else {
      group.push(Number(key));
    }
  }
  return [...groups.entries()]
    .map(([delayMs, ids]) => ({ ids: [...ids].sort((left, right) => left - right), delayMs }))
    .sort((left, right) => left.delayMs - right.delayMs);
}

/// 标记这些消息的正文正在拉取。已在途或已缓存的 id 不改。
export function begin(cache: BodyCache, ids: number[]): BodyCache {
  return update(cache, ids, (entry) => {
    if (entry?.phase === 'pending' || entry?.phase === 'cached') {
      return null;
    }
    return { phase: 'pending', attempts: attemptsOf(entry) };
  });
}

/// 写入批量拉取的正文。只接受在途的 id：已失效或快照重载丢弃的响应不写缓存。
export function arrive(cache: BodyCache, ids: number[], bodies: MessageBody[]): BodyCache {
  const accepted = new Set(ids.filter((id) => cache.entries[id]?.phase === 'pending'));
  if (accepted.size === 0) {
    return cache;
  }
  const entries = { ...cache.entries };
  for (const body of bodies) {
    if (accepted.has(body.id)) {
      entries[body.id] = { phase: 'cached', parts: body.parts };
    }
  }
  for (const id of accepted) {
    if (entries[id]?.phase === 'pending') {
      entries[id] = { phase: 'cached', parts: [] };
    }
  }
  return { entries };
}

/// 拉取失败：记一次尝试。只有在途的 id 计入，迟到的失败不改状态。
export function fail(cache: BodyCache, ids: number[]): BodyCache {
  return update(cache, ids, (entry) =>
    entry?.phase === 'pending' ? { phase: 'failed', attempts: entry.attempts + 1 } : null,
  );
}

/// 放行重试：失败的条目回到待请求，尝试次数保留，预算不会被重复使用。
export function retry(cache: BodyCache, ids: number[]): BodyCache {
  return update(cache, ids, (entry) =>
    entry?.phase === 'failed' ? { phase: 'ready', attempts: entry.attempts } : null,
  );
}

/// 快照重载：把该 session 在途与失败的条目退回待请求，正文与尝试次数保留。
export function reload(cache: BodyCache, ids: number[]): BodyCache {
  return update(cache, ids, (entry) => {
    if (entry?.phase === 'pending' || entry?.phase === 'failed') {
      return { phase: 'ready', attempts: entry.attempts };
    }
    return null;
  });
}

/// 丢掉该消息的正文与尝试次数，使下一次渲染重新拉取。在途与失败记忆一并清除。
export function invalidate(cache: BodyCache, id: number): BodyCache {
  return forget(cache, [id]);
}

/// 丢弃这些消息的条目，用于 session 删除。
export function forget(cache: BodyCache, ids: number[]): BodyCache {
  const removals = new Set(ids);
  const kept = Object.entries(cache.entries).filter(([key]) => !removals.has(Number(key)));
  if (kept.length === Object.keys(cache.entries).length) {
    return cache;
  }
  return { entries: Object.fromEntries(kept) };
}

/// 逐条改写条目。`step` 返回 `null` 表示该条目不改；全都不改时返回原值。
function update(
  cache: BodyCache,
  ids: number[],
  step: (entry: Entry | undefined) => Entry | null,
): BodyCache {
  const entries = { ...cache.entries };
  let changed = false;
  for (const id of ids) {
    const next = step(entries[id]);
    if (next === null) {
      continue;
    }
    entries[id] = next;
    changed = true;
  }
  return changed ? { entries } : cache;
}

/// 条目已用掉的尝试次数。未请求或已缓存的消息为 0。
function attemptsOf(entry: Entry | undefined): number {
  if (entry === undefined || entry.phase === 'cached') {
    return 0;
  }
  return entry.attempts;
}
