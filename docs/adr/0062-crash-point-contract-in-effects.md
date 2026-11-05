# ADR 0062: 崩溃点契约进副作用库，矩阵自证覆盖与完备

- 状态：已接受。细化 ADR 0056；推翻 `#79` 模块划分表中「崩溃点枚举不在 `effects` 这一层」一句；补充 ADR 0060 的覆盖率边界
- 日期：2026-11-05

## 背景

ADR 0056 把「任意位置崩溃」降成「每个提交点重启一次」，但**遍历与记账**仍在测试侧：`apps/agent/tests/crash_matrix.rs` 的两个用例各写一遍循环，断言是 `assert!`，第一个失败就中止，后面的点与后面的断言都不再跑。矩阵因此给不出「一共检查了多少、哪些没检查」。

四条性质与三条支撑假设是这套证据的全部前提：

| 性质 | 内容 |
| --- | --- |
| P1 | 每个崩溃点重启后收敛（没有 running 的 run，队列为空） |
| P2 | 崩溃前已在库里的内容，重启后仍在 |
| P3 | 每个非幂等调用的外部执行次数不超过 1 |
| P4 | 每个工具类的每一次外部执行，都在一条已落库的「已下发」之后 |

| 假设 | 内容 | 当时的守法人 |
| --- | --- | --- |
| A1 | 全部持久化写都经过 `Store::commit` | `scripts/check-write-path.sh` |
| A2 | 每个提交恰好留下一份副本 | 只有 `prefix_snapshots` 在一个小场景里断言一次份数 |
| A3 | 恢复判定是树与 run 的纯函数 | `plan.rs` 的签名与随机树尾穷举 |

三条缺口当时无人知道：`EffectClass` 六类的 `matrix_scope` 全部返回 `Covered`，而三个场景只走到 `model_step`、`read_file`、`bash`。`write_file`、`edit_file`、`compaction` 从未出现。覆盖率断言一上线就是三条红。

`record_commit` 在副本写入失败时只记 `warn`，写命令照旧成功：A2 被打破时矩阵静默少测一个点，没有任何东西报红。

## 决策

**契约进 `crates/effects::matrix`。** 公开面是 `CrashPoint`、`Kind`、`Finding`、`Checks`、`Matrix`、`Report`、`coverage_findings`（`#79` 的模块划分表补一行，模块头照仓库约定写「负责什么、不属于它的在哪」）。

- `CrashPoint` 只要求 `label() -> String`。一个点可以是副本路径、提交序号或注入的故障编号。
- `Matrix::new(points, expected, per_point)`：点表、使用方独立的计数、每个点必须应答的坐标清单。
- `Matrix::sweep(self, check: AsyncFnMut(P) -> Checks) -> Report`：遍历全部点，一个点的失败不打断其它点。
- `Report::extend` 追加场景级结论与覆盖面缺口，`Report::merge` 并入另一个场景的报告。
- 库不认识 SQLite、不认识消息树、不认识领域错误类型，不碰文件、不做网络、不写日志、不计时。它只 `.await` 使用方给的 future，因此不引入运行时依赖，ADR 0059 的结论不变。

**失败成为数据，遍历之后一次断言。** `Kind` 是七项闭集：`Failed`（断言不成立）、`Unchecked`（该点没有应答这条坐标）、`Unregistered`（结论没有登记在坐标清单里）、`Stray`（点表以外的断言）、`PointMissing`（点数与提交次数不符）、`EffectMissing`（声明覆盖却没出现）、`ReasonEmpty`（声明别处覆盖却没写理由）。报告一次列全，测试侧只留一处 `assert!(report.is_clean(), "{report}")`。库内不出现 `panic!` 与 `assert!`：`clippy::panic` 在 `crates/*/src` 是拒绝，生产代码不允许 `#[expect]`。

**完备性也是断言，两个方向都算。** 少答一条坐标是报告里的一行 `Unchecked`；一条结论没有登记在坐标清单里，是报告里的一行 `Unregistered`。只做前者，等于把「忘了一条断言」从「安静地少测」换成「安静地丢掉一条已经写好的断言」，病没有治。`Kind` 因此是七项闭集。

**点数对账是报告的内建结构。** 夹具用 `Store::commits()` 与副本目录里的序号对账：枚举出的点是 `1..=commits()` 的子集。缺口由**夹具**算，`Store` 不加为测试存在的读方法。`crates/core/tests/prefix_snapshots.rs` 补一条确定性注入：先在副本目录放一个非空文件 `prefix-000001.sqlite`，`VACUUM INTO` 报 `output file already exists`，于是「复制失败」不依赖磁盘满或权限也能测。

**三条缺口用场景关闭，不标 `Elsewhere`。** `write_file` 与 `edit_file` 进既有的「多步与幂等工具」场景；`compaction` 新增一个场景，用 `EngineConfig::context_char_limit` 触发，与 `run_failure.rs` 同一条引擎路径。今天不存在真正「别处覆盖压缩提交点」的测试，标 `Elsewhere` 就是把缺口写成承诺。顺带修正一条既有断言：`assert_ne!(node.run_id.map(|_| ()), None, "节点既不挂在 run 上也不是压缩消息")` 的条件与文案相反，压缩节点的 `run_id` 为 `None`，加压缩场景之后它会误报。

**反向验证常驻。** 「把某个不覆盖的类别改成覆盖，矩阵立刻红」不能只靠一次手工动作。`crates/effects/tests/matrix.rs` 用两份本地类别表跑同一份代码：一份把某类标成 `Elsewhere`，报告全绿；另一份标成 `Covered`，报告立刻点名它。这条用例不依赖任何真实场景，毫秒级，随 `cargo test` 跑。领域侧另有一次手工翻转记录：删掉场景里的 `edit_file` 调用，真实矩阵必须报 `EffectMissing` 并点名它。2026-11-05 实测：报告为「覆盖 · 覆盖缺口：类别 edit_file 声明矩阵覆盖，但没有一个场景走到它」，把那行加回去之后回到全绿。

**类别清单由 `effect_table!` 生成。** 一份声明同时生成枚举、`ALL_CLASSES` 与稳定标签。ADR 0060 承认的「唯一靠测试而不是编译器守住的清单」到此结束：清单不再有第二个来源。`replay` 与 `matrix_scope` 保持手写的穷尽匹配，新增一类仍要有人表态。

**提交的输入收窄。** `Changeset` 搬进 `crate::store`，构造入口与登记方法改成 `pub(in crate::store)`，删掉 `Default` 派生。于是 `store` 之外的代码造不出一份变更，也就调不动 `storage.apply`。这是破坏性变更：`durable::Changeset` 不再出现在公开 API。诚实边界：可见性以模块树为粒度，`store` 的后代仍可构造它。

**许可证按值消费。** `ToolBox::execute` 收 `Dispatched<EffectIntent>`，不收引用。`perform` 已经把所有权交给工作体，因此改动只落在签名与一处调用点；`Dispatched` 不实现 `Clone`，第二次使用是 `E0382`。

**副本写入失败仍然只记日志。** 事务已经提交；把副本失败报成写失败，会让调用方重试一次已经生效的写，与 P3 直接冲突。缺口由点数对账说出来，不被修复，也不被掩盖。

## 「形式化」的边界

这张表是本条对用户要求「不满足要求就编译不过」的正式回答。**shell 脚本不计入编译期。**

| 主张 | 档次 |
| --- | --- |
| 没有许可证就不能执行；一次下发只产出一个结局 | 编译期（私有字段、无构造入口、不 `Clone`、按值消费） |
| 类别清单不漂移；新增一类必须表态重放与覆盖 | 编译期（宏单一来源 + 穷尽匹配） |
| `store` 之外造不出一次提交 | 编译期（模块可见性） |
| 判定只读树 | 编译期（签名不含句柄）+ 穷举证据 |
| 收敛（P1）、不丢状态（P2） | 运行期证据（有界等待、链前缀、随机树尾穷举） |
| 跨重启至多一次（P3） | 运行期证据（逐点计数） |
| 落库先于执行（P4） | 编译期（执行接口只收许可证）+ 运行期证据（矩阵对每一次外部执行打开它发生时刻的副本，核对那一条是「已下发」） |
| `Elsewhere` 理由非空 | 运行期证据（报告里的一行） |
| SQL 只在 `storage/` | 门禁脚本 |
| 恢复后每个点都应答了每条坐标 | 运行期证据（报告里的 `Unchecked`） |

结论：能钉进编译器的是**形状**；收敛、不丢、次数与「已经落库」是**事实**，只能由穷举的逐点证据承担。任何声称这些是编译期性质的写法都是夸大。

## 后果

- 矩阵从三个场景增到四个，实测约 1.2 秒（ADR 0056 的基线是三个场景 0.8 秒）。提交点数量没有单独记录，按场景数与既有基线估计在 70 至 80 之间，属于估算。
- `effects` 新增两个模块（`matrix` 398 行、`table` 37 行），无新增依赖；`deps.allow` 与 `external-channels.allow` 都不改。
- `apps/agent/tests/support/matrix.rs` 的 `expect` 全部改成返回值：夹具 panic 会打断整次遍历，那正是本条要消除的东西。
- 将来加一条断言：不加进 `per_point` 就会被报告点名「未断言」。将来加一类副作用：改宏清单之后，编译器在 `replay` 与 `matrix_scope` 报错。
- 破坏性变更：`durable::Changeset` 不再公开。仓内无使用者，提交信息标 `BREAKING CHANGE:`。
- 边界不变：外部副作用的恰好一次、进程悬挂、介质损坏不属于本条承诺，见 ADR 0056 的后果一节。

## 备选

- **拉式 API**（`points()` 逐个取、`record()` 交回结论）：能让库完全不出现 `async`。否决理由是它的论据不成立——异步闭包只需 `core::future`，不需要运行时；而拉式把循环留在使用方，「遍历只有一份」就只剩纪律。它的两个机制（完备性、点数对账）照抄进本条。
- **覆盖表态改用 `SCOPES` 常量表 + `Reason` 新类型**：空理由在常量求值时是 `E0080`，比报告里的一行更强。否决理由三条：覆盖表态出现第二个来源；长度一致要另加常量断言，而该断言在 `cargo check` 下不触发（实测只在 `build` 与 `test` 出现），编辑内环因此以为挡住了；运行期调用 `Reason::new("")` 仍是 panic 路径。
- **`CommitTicket` 见证**：`apply` 收一张只有 `store` 能造出的凭据。与「`Changeset` 收进 `store`」同一道边界，多一个参数与一处签名改动，因此选后者。
- **`Recovered` 见证**：让「跑过第 0 圈」成为开跑的通行证。前提不成立：`round_zero` 已在 `drive_run` 里无条件调用，没有可省的路径。
- **副本失败计数（`copy_failures`）**：与点数对账功能重叠；对账还能抓到外部删掉一份副本，计数抓不到。
- **`Store::missing_commit_points()`**：把对账放进生产库。否决：夹具已经读目录，生产代码不必为测试增加一个读方法。
- **标 `Elsewhere` 关闭三条缺口**：把缺口写成承诺，而且「把不覆盖改成覆盖就立刻红」会退化成「改一个字符串」。
- **`trybuild`**：能校验错误码与快照，代价是新依赖与一整套 `tests/ui`。现有的 `compile_fail` 文档测试已经抓得住「有人给 `Dispatched` 加 `Clone` 或加构造入口」。
- **模型检验（TLA+ 或 Stateright）**：ADR 0056 已否决过一次，模型会与代码漂移。
- **每点 `catch_unwind`**：把夹具缺陷吞成断言失败；而且本机 panic 无法展开（`failed to initiate panic, error 5`，SIGABRT）。因此选「夹具不 panic」，不用「吞掉 panic」。

## 待定

- 取消与插话场景仍缺确定性的注入点，矩阵仍不覆盖它们。
- `compile_fail` 文档测试只断言「编译失败」，不断言错误码。这条限制来自候选设计在 `/tmp` 试验工程里的探针（标 `E9999` 也通过），本仓库没有自己的探针；`crates/effects/src/token.rs` 的两条既有用例同样只断言「编译失败」。因此它能抓「加回去」，不能抓「因为别的错误而失败」。
- 副本数量随场景增长，是否需要按形状去重（ADR 0056 的待定项）。
- 逐点重启会打开副本并让恢复写回它们，因此任何「读原始副本」的断言（例如 P4 的证据核对）必须在遍历之前跑。这条约束现在只写在夹具里，没有机制强制。
- 「副本缺口 → 报告里有 `PointMissing`」现在分两半证明：`prefix_snapshots` 证明缺口会出现，`effects` 的用例证明点表短了会报；没有一条用例让 `walk` 真的碰到缺口。
- `effect_limit` 用 `at_commits <= 序列号` 判定「崩溃前已发生」，这是夹具的近似：某个执行若发生在该提交之后，会被算进崩溃前。今天的场景不会假红，换场景需要重看。
- `EffectClass::of_tool` 仍是四个名字的数组，新增一类不会在那里报错；`is_tool` 与 `replay`、`matrix_scope` 是穷尽匹配。
- 门禁的规模预算与外部通道检查按 `git ls-files` 取文件表。文件改名之后，jj 重写的索引可能仍列出旧路径，脚本于是去读不存在的文件并失败（`check-source-budget.sh` 的 `wc`、`check-external-channels.sh` 的 `rg`）。临时修法是 `git add -A <目录>` 让索引与工作副本一致；更稳的修法是让两个脚本跳过工作副本里不存在的路径，那属于另一次变更。
- `engine/plan.rs` 的纯度门禁（禁止时钟与环境接口）：本条不做。要做就单开一条脚本与一条 ADR，不要塞进 `check-write-path.sh`——那条门禁的范围是单一写入线。
