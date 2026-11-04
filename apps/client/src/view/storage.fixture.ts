import type { KeyValueStorage } from './storage';

/// 内存实现的浏览器存储。用例用它断言写入与删除，不触碰真实的 `localStorage`。
export function memoryStorage(): KeyValueStorage & { entries: Map<string, string> } {
  const entries = new Map<string, string>();
  return {
    entries,
    getItem: (key) => entries.get(key) ?? null,
    setItem: (key, value) => {
      entries.set(key, value);
    },
    removeItem: (key) => {
      entries.delete(key);
    },
  };
}
