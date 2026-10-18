# ADR 0003: 多 workspace、多 session 与持久 FIFO 队列

- 状态：已接受
- 日期：2026-02-14

## 背景

用户要求多个 workdir 与多个 session 同时进行。

## 决策

workspace 以规范化绝对路径注册。session 绑定唯一 workspace。每个 session 有 FIFO 队列，队列项持久化，当前 run 结束后继续下一项，失败不阻塞队列。全局并发槽位默认 4。workspace 仍有 session 时拒绝移除。

## 后果

并发控制集中在调度器。队列项在开始执行时成为 active leaf 的子消息。
