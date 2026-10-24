# ADR 0039: 插话在步骤边界接入当前 run

- 状态：已接受
- 日期：2026-10-24

## 背景

`durable` 原先只有「入队」一种提交语义：run 进行中提交的消息等当前 run 结束后执行。用户看到答案走偏时只能取消再重发，丢掉已经完成的工作。规格没有覆盖这条能力，本 ADR 记录生效边界、降级规则与事实事件表示。

## 决策

**提交语义进协议**。`SubmitMessageRequest` 增加必填的 `mode`，取 `queue` 或 `steer`；省略 `mode` 的请求被拒绝。不加缺省值：旧客户端静默退化为排队比直接报错更难排查。`PROTOCOL_VERSION` 升到 4：旧节点忽略该字段，会把插话静默排队。两个方向都不兼容。

**插话在提交时就是链上的用户节点**。`mode=steer` 且该 session 有活动 run 时，`Store::enqueue_message` 立即在 active leaf 下写用户节点、把 active leaf 移到它，再写 `pending_steer = 1` 的队列项，`anchor` 指向该节点。没有活动 run 时插话退化为排队：不建节点，不移动 active leaf。队列项类型因此增加 `pending_steer` 字段。

**接入边界是步骤边界**。引擎在每次模型请求与 run 收尾之前调用 `Store::apply_pending_steers`：把所有待插入节点从链上摘下，再按队列项 id 顺序接到链尾，绑定当前 run、删除队列项，并把 active leaf 移到最后一个插话。工具执行期间不接入，正在执行的 `bash` 进程因此不被打断。接入后引擎重新读 transcript，尾节点成为用户消息，同一个 run 继续。

**顺序是「旧回答，然后修正」**。插话提交后定型的 assistant 消息挂在插话节点下面；摘下插话时把这些消息改挂到插话之前，再接上插话，链上顺序就不是「修正在回答之前」了。一次接入的写入顺序是拆出在前、接续在后，客户端逐条应用事件时不会看到父指针环。

**未消费的插话降级为排队**。run 结束（成功、失败或取消）后引擎调用 `Store::downgrade_pending_steers`：用同一套重排把插话接到链尾，清除 `pending_steer`，队列项保留作为下一次 run 的启动信号。取消语义不变：取消仍以 `cancelled` 结束 run，插话随后由下一次 run 执行。

**已送达的锚点从链尾接续**。降级后若先执行更早的队列项，插话节点会随链进入后续 run。轮到插话队列项时，`start_run` 发现它的锚点是「链上的用户节点但不是链尾」，说明正文已经送达模型，于是从链尾接续，不把 active leaf 拽回。该 run 通常立即收尾，不产生消息。

**事实事件**：`steer_applied { session, runId, itemId, messageId }` 表示插话已在该 run 生效；`steer_downgraded { session, item }` 表示降级。事件 id 单调，同一 run 内按发生顺序编号。接入是一次事务：`message_updated`（父节点与 run 关联）、`queue_item_removed`、`steer_applied`，必要时补 `active_leaf_changed`。既有事实事件的顺序语义不变。

## 后果

- 待插入插话在生效前就出现在 session tree 的 `nodes` 与 `queue` 里，刷新后仍在。客户端把它显示为一条用户消息，并在队列条上用徽标区分。
- 插话不打断工具：工具结束后，模型请求同时看到工具结果与插话。
- 插话只按提交顺序插入，没有优先级。
- 降级后插话可能被更早的队列项提前带入后续 run；轮到它自己的队列项时产生一个不含消息的 run。
- 删除待插入队列项后，插话节点留在树上，成为没有 run 的分支。
- 提交响应仍是 `QueueItem`，`pendingSteer` 说明它是待插入插话。
- 提交消息必须写明 `mode`。不带该字段的客户端收到协议错误，不再退化为排队。
