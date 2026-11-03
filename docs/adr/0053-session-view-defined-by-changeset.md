# ADR 0053: session 视图的定义只来自提交处的变更清单

- 状态：已接受。细化 ADR 0042 的「入口是唯一的视图推导点」，补充 ADR 0013 的队列长度约定与 ADR 0046 的不变量用例清单
- 日期：2026-11-03

## 背景

session 视图里有两个字段由别处的行推导而来：`queue_length` 是队列项行数，`busy` 是「存在 status 为 `running` 的 run」。这条规则此前有四个副本：

1. `store/command.rs` 的增量改写：`InsertQueueItem` 加一、`DeleteQueueItem` 减一、最后一条 run 变更决定 `busy`。
2. `SESSION_HEAD_SQL`：两个子查询，命令入口按 session 读。
3. `snapshot()` 里内联的同一段子查询：bootstrap 读全部 session。
4. `created_view`：新建 session 时硬编码 0 与 `false`。

副本 1 与副本 2、3 的关系不是抄写，而是「按行计数」的增量改写。因此两者的分歧只可能出现在增量一方。增量一方的缺口是两处兜底分支：`_ => 0` 与 `_ => {}`。新增一种会改变队列长度或 run 状态的变更种类时，编译器不发声，`command.rs` 只能把这条同步义务写进文档注释。

副本 3 是纯粹的抄写，而 `SESSION_HEAD_SQL` 上方的注释写着「全量快照与命令入口共用，两处计数因此一致」。这句注释只对了一半：命令入口用常量，全量快照用内联文本。

ADR 0042 的后果段声称「信封的视图与提交后快照逐字段相等，这条不变量由 `crates/core/tests/command.rs` 覆盖」。实际存在的唯一断言只覆盖新建 session 的零点（`tests/command.rs:339`）。提交后的一致没有用例。

推导还依赖一条既有前提：`start_run` 在 session 已有 `running` 的 run 时返回 `None`（`store/run.rs:40`）。因此「最后一条 run 变更决定 `busy`」与「存在 `running` 的 run」指向同一结果。

## 决策

**一、推导是唯一权威。** 新增 `SessionView { queue_length, busy }`（`session.rs`）：它是视图里由变更推导的那部分，`SessionMetadata` 仍是「记录加视图」的完整元数据。新增 `Changeset::session_view(&self, session: &SessionId, base: SessionView) -> Result<SessionView, StoreError>`（`change.rs`），与 `Change` 枚举相邻。内部逐个列出每个变体的影响，`match` 不带兜底分支：新增变体时编译失败，作者必须表态。基线被减到零以下或超出 `u32` 时报 `StoreError::Invariant`，提交失败。

**二、`updated_at` 不属于视图。** 它由「记录是否变化」决定，不由变更清单决定。规则留在命令入口，并与 `update_session` 的登记同处。新建 session 的视图是空基线上的推导，`created_view` 因此删除；副本 4 不再存在。

**三、SQL 只读回，不定义视图。** `session` 表上的两个计数收进一个名为 `session_head` 的 SQL 视图，定义折进 `crates/core/migrations/0001_baseline.sql`。`session_head()` 与 `snapshot()` 都从它 SELECT，`rows.rs` 的列名解码不变。视图定义里显式选出 `session.rowid AS row_id`，因为 bootstrap 的排序是 `updated_at DESC, rowid DESC`。

**四、视图只在提交处产出。** 删除 `SessionCommand::view()`。新增 `Store::command_view`，返回 `(T, Option<SessionMetadata>)`；`Store::command` 是它的薄包装。命令体不再有「按当时已登记的变更推导」的机会，因此也不会返回与信封不同的视图。

**测试。** 新增 `crates/core/tests/view.rs`：用固定种子的线性同余从公开写命令的菜单反复取值（入队、插话、开始 run、结束 run、重命名、编辑用户消息、切换 active leaf、删除 session 并新建一个），每步之后断言三处一致——事件信封的视图、`session_tree` 读回的视图、`snapshot()` 读回的视图。不引入 `proptest`：为一条不变量新增依赖不划算。

## 代价

- **一条规则仍有两份实现。** 推导与 SQL 的行计数并列存在。消除它只有两条路：存储返回行，或视图落库。两者都比现在的代价高，见「备选」。因此这份一致由一条用例守，不由结构保证。结构能保证的只是「新增变更必须表态」。
- **用例只能守一致，不能守定义。** 「`pending_steer` 的队列项算不算队列长度」这类问题仍由术语表与对象用例回答。
- **没有增量迁移路径。** schema 变更一律折进单一基线，旧库由 `verify_baseline` 拒绝打开。新增一个 SQL 视图也按这条规矩办，不接受热升级。
- **命令入口从三个变四个。** 术语表的「命令入口」与 ADR 0042 的描述同时更新。
- **扫描用例的失败信息比收缩过的属性用例粗。** 失败时给出步号与命令菜单里的取值，不给最小反例。

## 备选

- **存储只返回行，推导同时服务入口与快照。** SQL 不再出现计数规则。代价是 bootstrap 要为全部 session 读出队列项与 run 行；且 `WHERE status = 'running'` 仍是「`busy`」规则的一部分，只是从聚合挪到了谓词，收获得不到全额对价。
- **视图落库为两列。** `session` 表新增 `queue_length` 与 `busy`，在同一事务内由变更清单推着走；快照退化为普通 SELECT，读侧不再有任何规则的影子。代价是反规范化与一次基线变更，且崩溃后快照不能自愈：行里存的值错了就永久错。
- **引入 `proptest`。** 生成器与失败用例收缩更省心。代价是新增 dev 依赖，`deps.allow` 加行、`cargo deny` 通过、每个测试目标重复编译。
- **保留 `SessionCommand::view()`，用注释约定调用时机。** 推导是纯函数，两处调用结果必然相同。代价是把一条时序义务留在命令体里，而这是本次要消掉的那类义务。

## 后果

- bootstrap 与命令入口共用一份 SQL 定义，`SESSION_HEAD_SQL` 与内联子查询不再可能走偏。
- 新增会改变视图的变更种类时，编译在推导处失败，不依赖读者发现注释。
- 信封视图与读回视图的一致由 `crates/core/tests/view.rs` 断言；ADR 0042 声称而缺席的那条不变量补上了。
- 只有一处命令需要视图返回值（`rename_session`），它改用 `Store::command_view`。
- 术语表更新：`session 视图` 说明定义来源，新增 `视图推导`，`命令入口` 增列 `command_view`。
