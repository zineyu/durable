# ADR 0013: 队列项与自动标题

- 状态：已接受
- 日期：2026-10-24

## 背景

规格要求用户消息先入队，run 开始时才成为消息节点；首条用户消息决定 session 标题。原实现把准入直接当作建节点，队列长度恒为 0，自动标题缺失。

## 决策

**队列项是独立领域对象**。`queue_item` 表存 `id`、`session_id`、`text`、`created_at`，session 级 FIFO 按 `id` 升序。队列项身份由 `meta.next_queue_item_id` 全局分配器产生，分配先于提交，失败留空洞、永不复用。`MessageId` 与 `QueueItemId` 是两个独立空间，队列项执行时再分配消息 id。

**准入只入队**。`Store::enqueue_message` 只写队列项与 `message_queued` 事实事件，更新 `updated_at`；不创建消息节点，不移动 active leaf。节点创建推迟到 run 开始，由后续切片实现。

**标题自动更新由 `session.title_auto` 控制**。该列默认 1，重命名置 0。入队时若 `title_auto`、该 session 尚无消息且队列为空，标题取消息文本首个非空白行的前 50 个字符；否则保持标题。

**队列长度是派生值**。`SessionMetadata.queue_length` 由 `queue_item` 计数得到，`busy` 在 run 切片前保持 false。删除 session 时 `queue_item` 由外键 `ON DELETE CASCADE` 一并删除。

**新事件**：`message_queued { session, item }` 与 `queue_item_removed { sessionId, itemId }`。前者携带更新后的 session 元数据；后者由客户端 reducer 递减队列长度。

## 后果

- `POST /api/sessions/{id}/messages` 返回 201 与 `QueueItem`；消息树在入队后保持不变。
- 重启后队列、队列长度与自动标题保持一致。
- 队列项删除命令与 run 生命周期在后续切片接入，事件形状已固定。
