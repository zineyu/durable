# ADR 0052: 一次工具调用只有一个身份与一种终止

- 状态：已接受。细化 ADR 0015 的工具契约，删去其中 `NoTools` 的保留；补充 ADR 0049 取消翻译点的工具一侧
- 日期：2026-11-03

## 背景

规格 user story R-36 要求 link 断开时正在等结果的 run 以 `host_unreachable` 结束；规格 user story R-37 要求那次调用的结局是「结果未知」。

同一次调用在四个 module 里各有一个名字：`plan::PendingTool`、`ToolCall`、`Operation::Tool` 的三个字符串，最后是帧。`PendingTool` 与 `ToolCall` 字段相同，只是前者多一个树节点位置，于是每一跳都重建一次。

断链有两条通道：`ToolErrorCode::LinkClosed` 是一个没有任何构造点的错误码，真实断链走旁路 `Err(LinkClosed)`，再由引擎翻译成「结果未知」加 `host_unreachable`。`NoTools` 为测试保留，却没有调用者。executor 回答了错误的答复种类时，link 把它写成 `ToolOutcome::Failed{ io_error }`，于是链路协议出错被持久化成一次工具失败并回传给模型。

取消的落点已由 ADR 0049 收到一处；本 ADR 只改工具执行契约本身。

## 决策

**一个 `ToolCall` 贯穿引擎与传输。** `ToolCall` 含调用 id、工具名与参数 JSON，放在 `crates/core/src/tool.rs`，与 `ToolDefinition`、`ToolOutcome` 同处一个契约文件。它不携带消息树里的节点位置，也不派生 `TS`：树的簿记属于引擎，位置由 `plan::PendingCall { call, node }` 保存。帧不随它改变：`Operation::Tool` 仍是三个字符串，调用 id 与帧 id 无关，拆装只发生在 link 边界。

**执行契约有两种终止，且不是 `Result`。** `ToolBox::execute` 返回 `ToolEnd`：`Ended(ToolOutcome)` 或 `HostGone(LinkClosed)`。用枚举而不是 `Result` 的理由是 `?`：原来的旁路正是 `?` 让 `LinkClosed` 静默穿过工具循环一层。非 `Result` 的返回类型让调用点只能显式匹配两支。

**翻译只做一次。** 得到工具结局时写 tool 节点；宿主丢失时把在途调用与剩余调用都写「结果未知」，并让 run 以 `host_unreachable` 结束。这段翻译只在 `engine/tools.rs` 出现一次。

**删除 `ToolErrorCode::LinkClosed` 与 `NoTools`。** 前者没有构造点，它的存在让「不是工具结局的东西」有了结局的形状；后者没有调用者。`ToolBox` 的方法面因此收成 `definitions()` 与 `execute()`：幂等是工具定义上的标记，由引擎从定义表读，不再是被执行端回答的问题。

**传输事实不写进工具结局。** executor 回答了错误的答复种类时，link 返回 `LinkClosed::unexpected_reply`；workspace 校验的同类写法一并改，因为对端答错请求种类说明这条链路已不可信。

**新增 seam 六**：真实 `Store` 与临时 SQLite，加一个替身执行端，直接驱动 `RunEngine`。替身替换的是 host 侧执行端，不是被测的循环。

## 备选

- **保留 `Result<ToolOutcome, LinkClosed>`，只删错误码**：改动最小，但 `?` 仍能把宿主丢失静默传给上一层，旁路的成因没有被移除。
- **把 `node` 放进 `ToolCall`**：只留一个结构，但执行端会拿到一个自己永远不读的字段，帧里也没有它。
- **改帧为携带 `ToolCall`**：名字彻底统一，代价是帧协议版本升到 3，全部远端 host 变 `incompatible`，需要重新安装 executor。拆装发生在 link 边界已经足够。
- **保留 `idempotent()`**：默认实现与 `RemoteTools` 的重写今天同语义，保留它只多一个可以走样的重写点。

## 后果

- 「剩余调用写结果未知」第一次可以被断言，不再需要起 executor 子进程；端到端的断链路径仍由 `link_failure.rs` 覆盖。
- 术语表里「工具结局」与「调用终止」分工：前者持久化、上帧并回传模型，后者是执行契约的返回。
- 删除 `ToolErrorCode::LinkClosed` 改变生成的 TS 类型与 `docs/api/types.md`，必须运行 `durable-typegen` 并提交生成文件的变更。该取值没有构造点，历史数据里不存在它；对协议消费者仍是公开取值的删除，提交信息标记 `BREAKING CHANGE:`。帧协议版本不变，远端 executor 不需要重装。
- ADR 0015 中「`NoTools` 保留」一句删除；ADR 0007 的 seam 清单增加 seam 六。
- 取消的落点仍按 ADR 0049：工具循环返回 `ToolLoopEnd::Cancelled`，由执行循环翻成 `cancelled()`；本 ADR 只改它下游的 `ToolBox` 边界。
