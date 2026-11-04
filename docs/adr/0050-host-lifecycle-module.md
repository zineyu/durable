# ADR 0050: host 状态机与 link 生命周期收进一个 module

- 状态：已接受。细化 ADR 0048 的「状态集中」，并关闭其中安装期间停用的缺口
- 日期：2026-10-30

## 背景

一台 host 的状态写入分散在三处：连接任务（`hosts/connect.rs:35`、`:65`、`:75`）、link 关闭的翻译（`hosts/mod.rs:404-425`）、停用与改目标（`hosts/mod.rs:340-359`）。状态说明是自由字符串，于是「不是 `ok`、但也不是失败」的四种情形只能靠文案区分：`DISABLED_DETAIL`、`CONNECTING_DETAIL`、快照回退分支里的「尚未连接」，以及 `HostRuntime::disconnected()` 给出的 `None`。同一个「还没连上」因此有三种文本与两种字段取值。

代号的配对规则分居两个文件：连接任务在 `connect.rs:64` 取号，关闭通知在 `mod.rs:410` 比对，只有两半都对才生效。回退文案在 `hosts/mod.rs:249-253` 与 `api.rs:380-388` 各写一份。一次断链的账要跳四个文件才看得全。

更实际的问题是这条分散带来一个现存缺口：`supervisor` 只在循环开头读一次 host 行，而 `ssh::prepare`（探测与安装）不在取消的 select 之内。若用户在 prepare 期间停用 host，prepare 返回后仍然会写入 `ok` 并挂上 link，随后取消分支立刻醒来让任务退出，`runtime` 里却留着 `ok` 与一条活的 link，而此后没有任何路径再纠正它。

## 决策

**一个 module 拥有这台 host 的全部进程内状态。** 新增 `hosts/lifecycle.rs`，类型 `HostLifecycle`，持有状态、link、代号、心跳延迟与后台任务的取消通道。名称不用 `HostSession`：术语表里 `session` 已经是「绑定唯一 workspace 的对话单元」，同一个词根指两件事会让代码与文档互相误导。

**`apply` 是同步方法，事实写入留在 registry。** `apply(&mut self, event) -> Applied` 只改字段并回答三件事：事件是否被采纳、变化之后的状态、签发的代号与取消接收端。`host_status_changed` 的写入只有 registry 一处调用点，在锁外执行。状态决定只有一处，事实写点只有一处。

**状态与说明合成一个枚举。** `HostState { Ok(Arc<LinkHandle>), Connecting, NeverConnected, Disabled, Failed(HostFailure) }`，`HostFailure` 取 `LinkLost(LinkClosed)`、`ProtocolMismatch(LinkClosed)`、`Incompatible(String)`、`Unreachable(String)`。`status()` 派生 `HostStatus`，`explanation()` 派生文本，`Ok` 时为 `None`。「`ok` 却带说明」与「非 `ok` 却无说明」不再可表示，三种「还没连上」的文本合成一种。「有没有变」按 `(status, explanation)` 比较，不按枚举相等比较，否则 `Ok(a)` 与 `Ok(b)` 会被判成变化。

**`Ok` 携带 link。** 「`ok` 必有能用的 link」由类型保证，`HostRegistry::link()` 不再需要 `is_ok()` 守卫。

**陈旧事件由 module 内签发的代号丢弃。** `apply(Connecting)` 签发不透明的 `Attempt`，registry 把它交给连接任务；`Opened`、`AttemptFailed`、`Close` 与 `set_latency` 全部带上它，module 内比对一次。停用与新一次重建只做一件事：让旧代号失效。这条规则同时覆盖「停用之后才到达的 `Opened`」「被替换后到达的 `Close`」「被替换后到达的延迟样本」，上面那个缺口由构造成立。

**过渡状态只给 ssh。** `Connecting` 对 ssh host 进入「正在连接」，对本机 host 只作废旧代号，不改状态与说明。本机建链是本地动作，毫秒内完成；为它写一条过渡事实只会让每次启动与重连各多一条历史记录。这条规则依据 host 行的 `kind`，与今天的行为一致。

**三态判定只有一处。** 事件携带原因，`HostState::status()` 是唯一判定点；连接任务只回答「这次尝试怎么失败了」，不回答「这是哪个状态」。`link::ssh::SetupFailure` 保持原样，由 `connect` 翻成原因。

**事件集五个。** `Connecting`、`Opened`、`AttemptFailed`、`Close`、`Disabled`。没有 `TargetChanged`：改目标、手动重连与首次注册在生命周期看来都是 `Connecting`。没有 `Removed`：移除 host 就是让表项消失，之后到达的事件因为找不到表项被丢弃。

**文案实现一份，调用三处。** `HostState::explanation()` 是唯一实现；`protocol` 的快照、`api` 的不可用说明与 `LinkClosed::unavailable`（工具失败文本）各自调用它。`api.rs` 与 `hosts::unavailable_detail` 的重复回退文案合成一处。

**事实载荷不变。** `host_status_changed` 仍写渲染后的 `Option<String>`。内核不引入连接层的失败词汇，客户端类型与协议版本不变。

**心跳延迟不走事件。** 延迟不改变状态，由 `set_latency(attempt, ms)` 写入；它与 `apply` 同在一个 module，共享同一条代号比对规则。

## 备选

- **保留今天的 `generation`，只把两半配对搬进 module**：改动最小，但「取号」与「比对」仍是两段代码，读的人要继续确认它们对得上。
- **用 `Arc::ptr_eq` 判定陈旧，另设 `Disabled` latch 拒绝迟到的打开**：不引入代号，但 latch 是 host 行 `enabled` 的第二份副本，而且需要同时维护两条规则。
- **`apply` 是 async 且持有 `Arc<Store>`，自己写事实**：module 内只有一条路径，但单测从此需要数据库，「不带 IO 的状态机」这条测试面消失。
- **把状态原因放进 `durable` 内核，事实改带结构化原因**：读历史能按原因如实重放，文案改动不影响旧事实。代价是内核多一套 ssh 与 executor 的失败词汇，事实载荷加字段要处理旧事件的缺省值，协议版本与客户端类型都要动。
- **保留 `status` 与 `Option<detail>` 两个字段**：改动更小，但非法配对仍可表示，重复文案要在实现里逐个收拾。
- **让 `protocol` 成为唯一渲染点，`api` 与 `link` 向它取文本**：`hosts` 已依赖 `protocol::HostSnapshot`，反向再依赖会成环。
- **保留 `TargetChanged` 事件**：文档上能直接读出「改目标」这个用户动作，但生命周期必须复制 host 行的 `target`，而它并不拥有该字段。

## 后果

- 安装期间停用 host 的缺口关闭：安装完成后到达的 `Opened` 因代号已失效被丢弃，状态停在已停用，link 被丢弃。
- 尚未连接的 host，快照里的说明由 `null` 变成「尚未连接」。客户端原本在 `null` 时显示状态标签，因此没有视觉回归，只有文案更具体。
- 事实序列不变：本机 host 启动仍只写两条 host 事实，重启后的事件计数不变。
- 重放历史时看到的是写入当时的文案；文案改动只影响此后写入的事实。
- `hosts/mod.rs` 缩小为注册表：表项、命令、快照与心跳；`connect.rs` 只报告一次尝试的结果。
- 不新增跨进程 seam：测试是 `lifecycle.rs` 的模块内单测（枚举事件序列）与既有的 `apps/agent/tests/hosts.rs`。后者需要一条不拉子进程的 `LinkHandle`，由 `#[cfg(test)]` 的构造函数给出。
- `docs/glossary.md` 新增「状态原因」与「连接尝试」，并把「状态说明」改为「状态原因的渲染」。
