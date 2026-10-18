# ADR 0004: 消息树与分支历史

- 状态：已接受
- 日期：2026-02-14

## 背景

编辑历史消息与重试需要保留旧历史并可切换。

## 决策

session 的消息组织为树，session 保存 active leaf。编辑用户消息在相同父节点下创建兄弟节点；重试 assistant 消息在其父节点下新建 run。两者都要求 session 空闲且队列为空。分支切换只移动 active leaf。不提供单分支删除，删除 session 删除整棵树。

## 后果

transcript 始终是 root 到 active leaf 的路径。所有会话状态由 server 推导，客户端不保存权威状态。
