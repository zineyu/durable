/// 浏览器键值存储的最小接口。测试传入内存实现，生产传 `window.localStorage`。
/// 草稿、上次打开的 session 与 transcript 滚动位置都写在这类存储里。
export type KeyValueStorage = Pick<Storage, 'getItem' | 'setItem' | 'removeItem'>;
