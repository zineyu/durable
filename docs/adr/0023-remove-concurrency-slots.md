# ADR 0023: 移除并发槽位

- 状态：已接受
- 日期：2026-11-03

## 背景

主规格设定「全局并发槽位默认 4，可配置」，代码用 `tokio::Semaphore` 实现，配置项为 `DURABLE_MAX_CONCURRENCY`，并有测试 `enforces_global_concurrency_slots_across_sessions`。引入多节点后，「全局」的边界含糊：槽位属于 `hub` 还是每个 node？用户已决定移除限流。

## 决策

删除并发槽位概念。

- 删除 `max_concurrency` 配置与 `DURABLE_MAX_CONCURRENCY`。
- 删除 `Engine` 的信号量与等待槽位的路径。队列项被取出即启动 run。
- session 内的 FIFO 队列仍然串行化自身的 run。这个串行是领域规则，不是资源限制。
- 删除 `enforces_global_concurrency_slots_across_sessions` 测试。

## 备选

- **每节点各 4**：`agent` 不变，但多节点下用户会误以为总并发仍是 4。
- **`hub` 全局限流**：`hub` 必须感知各 node 在跑的 run，等于持有领域状态。
- **保留可配置项、默认无限**：概念残留，且要定义「无限」的配置写法。

## 后果

- 并发上限由模型供应商的限流与机器资源自然施加。超出时 run 以显式的 `model_upstream` 失败结束，队列继续。
- 调度器变简单：取出队首即执行，没有等待槽位的状态。
- `run` 状态集合不变，`queued` 一直由队列项承载，从不写入 run 表。
- 同一 session 的严格串行仍是唯一保证；跨 session 的执行顺序不再有调度承诺。
