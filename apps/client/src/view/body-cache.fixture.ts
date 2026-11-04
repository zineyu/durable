/// 本模块负责测试共用的正文缓存构造：按消息 id 给出已缓存的正文。
/// 阶段与尝试次数的构造走 `body-cache` 的动作，这里只省去重复的 `begin` 与 `arrive`。
import type { ContentPart } from '../api/generated/ContentPart';
import { arrive, begin, emptyBodyCache } from './body-cache';
import type { BodyCache } from './body-cache';

/// 夹具里的固定请求令牌：这些构造不模拟迟到的响应。
const TOKEN = 'fixture';

export function cacheOf(bodies: Record<number, ContentPart[]>): BodyCache {
  const ids = Object.keys(bodies).map(Number);
  const messages = ids.map((id) => ({ id, parts: bodies[id] ?? [] }));
  return arrive(begin(emptyBodyCache, ids, TOKEN), ids, TOKEN, messages);
}
