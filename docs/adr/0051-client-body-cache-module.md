# ADR 0051: 正文缓存收进一个 module

- 状态：已接受。细化 ADR 0038 的正文按需拉取与 ADR 0044 的派生层读取边界
- 日期：2026-10-30

## 背景

正文缓存由四个 module 各管一段。

- `bodies.ts` 用三个平行字段写七个迁移函数，并在每个函数里维持「一条消息最多出现在 `bodies`、`bodyRequests`、`bodyFailures` 之一」这条互斥规则。
- `derived.ts:123-137` 为判定缺失把这条规则又写一遍：`bodies[id] === undefined && !bodyRequests.includes(id) && !bodyFailures.includes(id)`。
- `state.ts:263-264` 在装载一棵 session 树时清空 `bodyRequests` 与 `bodyFailures`，清空范围是全部 session，而装载的只是一棵。
- `use-message-bodies.ts` 自己持有尝试计数 `failuresRef` 与延时常量 `RETRY_DELAYS_MS`，失败记忆却在 `bodies.ts`。

于是同一个问题有两个答案来源：规则有两份（`bodies.ts` 的七个写点与 `derived.ts` 的一处重述），策略有两份（hook 的计数与 `bodies.ts` 的失败集合）。要回答「一条消息的正文什么时候会重新拉取」，要在五个 module 之间来回跳。

此外，今天的计数只在 session 切换与任一批次成功时归零。一条持续失败的请求会被其他消息的成功无限期续命：计数归零，定时器再次触发，请求再次失败。

## 决策

**一个 module 持有条目表与重试预算。** 新增 `client/src/view/body-cache.ts`，类型 `BodyCache`，内部是 `Record<number, Entry>`；`Entry` 是带阶段的联合：`ready(attempts)`、`pending(attempts)`、`cached(parts)`、`failed(attempts)`，没有条目即「未曾请求」。互斥规则不再存在，因为一条消息在一个时刻只有一个阶段。

**interface 只有动作与纯读。** 动作七个：`begin(ids)`、`arrive(ids, bodies)`、`fail(ids)`、`retry(ids)`、`invalidate(id)`、`forget(ids)`、`reload(ids)`。纯读三个：`needs(ids)`、`cachedParts(id)`、`retrying()`。`Entry` 与条目表不导出。

**`needs(ids)` 收候选 id。** 活动链的回溯留在 `derived.ts` 的 `chainOf`。缓存不认识 session，也不读 `SessionTreeState`，因此不成为 ADR 0044 所说派生层之外的第二个分片读者。`needs` 返回条目为空或为 `ready` 的 id。

**尝试次数按 id 记账。** `fail(ids)` 给每个 id 加一；`arrive(ids)` 与 `invalidate(id)` 清零；`forget(ids)` 丢弃。上限为三次，即首次请求加 `RETRY_DELAYS_MS` 的两档。`retry(ids)` 把 `failed(a)` 变成 `ready(a)`，计数随阶段保留，因此预算既不会重复使用，也不会在重试时丢失。行为变化：一条持续失败的消息不再被其他批次的成功续命。

**`retrying()` 按等待时长分组。** 纯读返回 `{ ids, delayMs }[]`，同一等待档位的消息构成一组。hook 为每组设一个 timer，到点后一次 `retry(ids)`，随后由 `needs` 的 effect 一次重发。时间效果留在 hook，策略留在 module。

**`reload(ids)` 只回退该 session 的条目。** 快照装载把该 session 的 `pending` 与 `failed` 退回 `ready` 并保留计数，`cached` 保留，其他 session 不动。行为变化：装载 session B 不再作废正在显示的 session A 的在途请求。代价是重载时该 session 在途的响应当场作废，随后重发一次请求。

**占位与缓存分处。** `bodies.ts` 收窄为 `placeholders.ts`，只留 `applyDelta`、`dropPlaceholder`、`dropFinished`。占位按 session 记、随 run 结束丢弃，与「正文该不该拉」是两条生命周期。

**越界读取由审查准则发现。** 审查准则：`rg "\.phase"` 的结果只应出现在 `body-cache.ts` 与其测试里。与 ADR 0044 对分片表的规定同一做法，不新增门禁脚本。

## 备选

- **把三个平行字段只藏进 module**：改动面更小，与今天的代码逐段对应，审查容易对照。放弃它，因为互斥规则仍要写在七个函数里，规则只是换了位置。
- **重试预算留在 hook**：省掉条目表上的 `attempts`。放弃它，因为「失败记忆在状态、尝试计数在 hook」正是本次要消除的撕裂，且 hook 拿不到刚写入的计数。
- **`needs(sessionId, trees)`，缓存自己回溯链**：调用点更省事。放弃它，因为缓存会依赖 `SessionTreeState`，成为分片表的第二个读取者，而缓存本来按消息 id 与 session 无关。
- **把 `missingBodyIds` 整体搬进 module**：判定只有一处。放弃它，因为缓存要依赖 `ClientState`，而 `ClientState` 又持有缓存，形成自指。
- **全局 `reload()`**：与今天的行为完全一致，不必论证行为变化。放弃它，因为没有理由说明为什么装载一棵树要让别的 session 重发请求。
- **用 class 与 `#` 私有字段**：类型层直接挡住越界读取。放弃它，因为状态从纯值变成对象，测试要经动作构造而不能只写字面量，快照比较也要改写法。
- **新增门禁脚本检查字段名越界**：不靠自觉。放弃它，因为 `phase` 是常用词，白名单只能写成很窄的表达式，门禁本身成为需要维护的资产。

## 后果

- `derived.ts` 的 `missingBodyIds` 变成「取活动链上的 id，再问 `needs`」两步；取正文改经 `cachedParts(id)`。未缓存时 `cachedParts(id)` 返回 `null`，transcript 的 `parts: null` 语义不变。
- `state.ts` 不再持有三个字段，也不再维护重置清单；`message_updated` 与 session 删除各调用一个动作。
- 测试分两层：`body-cache.test.ts` 对纯值断言四条规则（失败后不再进入 `needs`、计数用尽后停止重试、`reload` 后重新进入 `needs` 且计数保留、不在途的响应被丢弃）；`reducer.test.ts` 保留三条经 `reduce` 的接线断言（快照装载、消息更新失效、session 删除丢弃）。
- 新增一个阶段的代价：编译器在 `body-cache.ts` 内的穷尽检查报错，其他 module 不改。
- 缓存仍然全局按消息 id 记。`reload(ids)` 的范围由调用者决定，模块不设 session 概念。
