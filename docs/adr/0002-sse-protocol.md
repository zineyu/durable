# ADR 0002: HTTP 命令 + 全局单一 SSE 事件流

- 状态：已接受
- 日期：2026-02-14

## 背景

客户端需要同时观察多个 session 的流式输出，双向需求只有用户命令。

## 决策

命令走 HTTP JSON；事件走单一全局 SSE 流，事件带全局自增 id 与 session_id。客户端用 `after` 游标断线续传。只持久化 fact event；delta 不落库，run 结束时写入完整 assistant 消息。bootstrap 只返回元数据与游标，消息树按 session 懒加载。

## 后果

协议是唯一的跨语言契约，用 ts-rs 生成并提交 TS 类型。多个标签页可以同时订阅并操作。增量输出在 run 未结束时只存在于 server 内存，通过 bootstrap 获取。
