# ADR 0016: 分支命令与 active leaf 切换

- 状态：已接受
- 日期：2026-11-08

## 背景

ADR 0004 定义消息树与分支，但不含命令实现。规格要求编辑历史用户消息、重试 assistant 消息与切换 active leaf；三者都只在 session 空闲且队列为空时可用。原实现只有「入队 → run 开始时创建用户节点与 assistant 节点」一条路径，没有分支入口。

## 决策

**队列项可以锚定一个已存在的用户节点**。`QueueItem` 增加 `anchor: Option<MessageId>`。`None` 表示 run 开始时在当前 active leaf 下新建用户节点；`Some` 表示接续该用户节点。`start_run` 因此统一处理三种来源：新提交、编辑、重试。

**编辑用户消息创建兄弟用户节点**。`edit_user_message` 在目标用户消息的父节点下创建新用户节点，把 active leaf 移到新节点，并入队一个锚定该节点的队列项。节点创建时 `run_id` 为 `None`；`start_run` 发现锚定节点的 `run_id` 缺失时补上本次 run，并追加 `message_updated` 事件。旧消息及其子树不变。

**重试 assistant 以父用户节点为锚**。`retry_assistant_message` 只入队一个锚定目标 assistant 父节点的队列项，不创建用户节点。`start_run` 复用该用户节点，创建同父的 assistant 节点。原用户节点的 `run_id` 保持指向原 run。

**切换 active leaf 不产生消息**。`switch_active_leaf` 把 active leaf 移到目标子树中最近创建的叶子，即子树内 id 最大的叶子；消息 id 全局单调递增，因此 id 最大等于创建最晚。目标是叶子时即为 active leaf。该命令只修改 `session.active_leaf`，并追加 `active_leaf_changed` 事实事件。`FactEvent` 增加 `ActiveLeafChanged { session, active_leaf }`。

**前置条件是显式冲突错误**。`ensure_idle` 先检查队列非空，再检查 run 进行中；两者不满足分别返回 `queue_not_empty` 与 `run_in_progress`（HTTP 409）。目标消息不存在返回 `message_not_found`（404）；目标角色与命令不匹配返回 `message_role_mismatch`（HTTP 422）。

**路由**：`POST /api/sessions/{id}/messages/{message}/edit`、`POST /api/sessions/{id}/messages/{message}/retry`、`POST /api/sessions/{id}/active-leaf`。

## 后果

- transcript 始终是 root 到 active leaf 的路径；分支切换后 `session_tree` 与 transcript 同时变化，重启后从 SQLite 恢复。
- 编辑与重试复用队列与 run 引擎，因此崩溃恢复、并发槽位与取消语义与普通提交一致。
- 队列项携带 `anchor`，客户端可见；分支切换的权威状态仍由 server 推导。
