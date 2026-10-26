# ADR 0043: 错误分类归内核

- 状态：已接受。细化 ADR 0014 的失败类别、ADR 0028 的状态码选型与 ADR 0031 的类别约定
- 日期：2026-11-05

## 背景

`durable::StoreError` 有 17 个变体，自身不带分类。三个调用方各自解释它，结论互不一致。

第一，agent 的 HTTP 错误映射把节点相关的两个变体连同存储与不变量变体一起落到 `Internal` 与 500，因为 agent 的 `ErrorCode` 里没有对应项。

第二，hub 的错误映射只显式处理节点相关的两个变体，其余变体经 `_ =>` 兜底落到 `Internal`。新增变体时编译器不提示，新增的变体因此会静默落到 500。

第三，引擎把任意 `StoreError` 压成 `RunFailureCode::ModelUpstream`：工具循环、transcript 读取、待插入插话、assistant 落库与消息 id 分配都走这条路径，压缩路径则把写库失败压成 `ContextLimitExceeded`。这条失败记录写进消息树的 run 记录并持久化，客户端再按失败类别渲染成「模型上游错误」。用户与运维看到的原因与实际原因不符，且只能靠日志找回真相。

## 决策

**分类分两层，都由内核给出。** 第一层是 `StoreErrorKind`：与 `StoreError` 的 17 个变体一一对应，不带载荷，由 `StoreError::kind` 穷尽给出。第二层是 `StoreErrorClass`：`NotFound`、`Conflict`、`InvalidArgument`、`Internal`、`StorageFailure`，由 `StoreErrorKind::class` 穷尽给出。第二层叫结局类别，只表达结局，不含 HTTP 词汇。

**HTTP 状态码只由结局类别决定。** agent 与 hub 各有一个从结局类别到状态码的穷尽函数，不按变体各自判断。两个 seam 的协议码各自穷尽给出，不保留 `_ =>` 或等价兜底。新增一个变体时，编译器依次要求三件事：`kind` 给出第一层分类、`class` 给出结局类别、两个 seam 的协议码函数给出各自的码。

**agent 的 `ErrorCode` 增加 `NodeNameTaken` 与 `NodeNotFound`**，状态码 409 与 404。hub 的 `HubErrorCode::UnknownNode` 同时改名为 `NodeNotFound`，两个 seam 对同一变体给出同一字面与同一状态码。虽然 agent 当前没有节点表的写命令，这两个变体在 agent 侧不可达，但仍要给码：结局类别已由内核统一，agent 改报 `Internal` 等于覆盖内核的决定，两层设计随之失效。改名不额外增加破坏性：协议版本本次已经递增。hub 的规格（`docs/specs/multi-node-hub.md`）中四处 `unknown_node` 字面同期改为 `node_not_found`。

**引擎内任何 `StoreError` 都记为新增的 `RunFailureCode::StorageFailed`**，失败说明使用 `StoreError` 的 Display 文本。压缩路径同样：写库失败记存储失败，模型侧失败仍记 `ContextLimitExceeded`。

**`Interrupted` 不复用为存储失败。** 它只为读出历史记录里的旧值而保留，不产生新值。理由有两层：旧值的含义是「进程中断」，与「本地存储失败」是不同原因；复用会让客户端与日志无法区分两类原因，而这正是本次要修的问题。

**协议版本在同一次改动里升到 6。** 三处常量同期递增：agent 协议版本、hub 的浏览器协议版本、hub 期望的 agent 协议版本。客户端 `runFailureLabel` 与 hub 的错误码都是穷尽 `Record` 与穷尽枚举，旧客户端遇到未知键会渲染 `undefined` 前缀的说明，因此这是破坏性改动。分两次升版没有收益：两个变体来自同一条决策，拆开只会多留下一个语义不完整的中间版本。

## 备选

- **在 `StoreError` 上加一个返回协议码的方法**：内核会知道 HTTP 词表与两个 seam 的码，内核与协议互相耦合。
- **只让结局类别决定状态码，不给 agent 补节点码**：agent 继续把节点错误报成 `Internal`，同一错误在两个 seam 得到不同解释，问题只修了一半。
- **把存储失败并入 `interrupted`**：见「决策」段，复用会丢掉原因区分。
- **两个 seam 各留 `_ => Internal` 兜底**：编译器不再强制给码，新增变体仍会静默落到 500。

## 后果

- `RunFailureCode` 沿用既有的 `snake_case` 序列化名，新类别是 `storage_failed`。任务说明里写的是 camelCase，与枚举既有约定不符，以既有约定为准。
- 启动 run 之前的写库失败与 run 收尾的写库失败没有可写的 run 记录，只能记日志。这两处保留现状，不引入第二种失败上报路径。
- 持久化的失败记录与原因一致：数据库、不变量与序列化失败都记为 `storage_failed`，说明带内核错误的原文。
- agent 与 hub 对同一变体给出一致的协议码与状态码，映射表只需读内核一处。
- 新增 `StoreError` 变体不再是三处手动同步，而是编译错误驱动。
- 破坏性协议变更：客户端需同时升级 `RunFailureCode` 与 `ErrorCode` 的标签表。
- 结局类别里 `Internal` 与 `StorageFailure` 当前都给 500，引擎也不区分两者：任何 `StoreError` 都记 `storage_failed`。分开保留它们是为了让内核的结局来源仍可见，并让将来给存储故障另选状态码时只需改一个函数；如果这条理由不再成立，两者应合并。
- hub 的写命令只有节点表，因此 workspace、session、队列与消息相关的分类在 hub 不可达，报 `internal`；存储类分类（`Database`、`CorruptedData`、`Serialization`、`Invariant`）在 hub 可达，同样报 `internal` 配 500。
- 由于状态码取自内核的结局类别、协议码取自各 seam，hub 可能返回 `internal` 搭配 404、409 或 422。这是两层分工的直接结果，不是遗漏；客户端判断「能否重试」应看状态码，判断「哪一层拒绝」应看协议码。
- 两个 seam 各有一份「结局类别到状态码」的穷尽函数与一个 `from_kind` 入口，内容相同。放在内核会引入 HTTP 词表，放在一处又会让另一个 seam 依赖对方的错误信封。两份相同实现由各自 seam 的表驱动测试钉住同一组状态码。
- 三份分类表（内核、agent、hub）是同源期望值的独立副本，不是待抽取的重复：每份钉住自己那一层的映射，任一层改了映射而没改期望值，对应测试即失败。
