# ADR 0044: 客户端派生视图层

- 状态：已接受。细化 ADR 0017 的「组件只接收派生后的 props」与 ADR 0024 的分片形状
- 日期：2026-11-06

## 背景

ADR 0017 要求全部业务状态由纯 reducer 推导，组件只接收派生后的 props。实际代码停在半步：协议 DTO 直接充当视图模型，组件收到整块分片 `NodeState`，每次渲染自己重新推导。

由此产生四个后果。

- 派生没有所有者。`SessionPane` 在渲染体里推导空闲判定、工具结局、每条消息的分支序号、transcript、占位与输入框历史；`App.tsx` 每帧重算侧栏分组、节点说明与运行中列表。
- 同一判定有多份实现。session 状态由 `sessionStateOf`、侧栏摘要里的内联三元式与 `SessionPane` 里的内联判定各写一次。
- 遍历全部分片的循环抄了四份：运行中列表、命令面板的 session 列表、正文拉取的缺失节点计算、主面板的派生调用。
- 派生无法单独测试。`reducer.test.ts` 有 1379 行，全部用例经 `reduce(initialClientState, bootstrap)` 造状态；侧栏排序一个 `toEqual` 要 74 行。

## 决策

**派生层是纯函数，不进 reducer。** 新模块 `client/src/view/derived.ts` 从分片推导呈现所需的视图，不持有状态，不产生事实。订阅分片的调用点按分片对象为键 `useMemo`，分片换代即重算。分片不可变，因此不需要失效逻辑，reducer 的语义不变。

**两级构造。** `sessionSummaries` 计算节点内全部 session 的摘要，代价足够低，必须全算，供侧栏、命令面板与运行中列表使用。`sessionView` 计算完整视图：transcript（消息连同正文、展示状态与说明、分支序号与相邻 id）、工具结局、队列、占位与空闲判定；代价与 transcript 长度相关，只算当前可见的 session。完整视图内嵌摘要，摘要的实现只有 `sessionSummaries` 一处。

**派生层是分片的唯一常规读取者。** 除 `derived.ts` 之外，任何模块不得为呈现目的读取分片字段，也不得遍历 `Record<NodeName, NodeState>`；跨节点的循环（运行中列表、命令面板列表、缺失分片）都在派生层内，一次遍历给出结果。组件只接收派生后的 props：`SessionPane` 接收 `SessionView`，不接收分片。

**按需查询，不做缓存。** 派生结果是返回对象，不是 reducer 字段，也不是模块级缓存。需要摘要的地方调用摘要函数，需要完整视图的地方调用视图函数；代价高的那一级只对可见 session 调用。按分片对象缓存只发生在调用点的 `useMemo` 上，因此同一帧内不重复构造。

**协议字段由类型保证流入分片。** `SessionTreeState` 改为 `Omit<SessionTree, 'session' | 'maxEventId'>`，装载函数取快照的剩余字段。协议新增字段时不必逐字段抄写，字段自动进入分片。

**选中位置只有一处所有者。** `hub.selected` 只写不读，删除它；`ClientAction` 的 `select-node` 一并删除。URL 哈希是选中位置的唯一所有者，初始化与 `hashchange` 共用 `readSelection`，写哈希只经 `navigate`。

**`App.tsx` 拆到规模预算以内。** 主面板分支与 `SessionPane` 组装进 `components/MainPane.tsx`；全部写命令处理器与对话框确认进 `view/use-commands.ts`。`App.tsx` 从 593 行降到 383 行，`scripts/source-budget.allow` 的登记行删除。

## 备选

- **把派生结果放进 reducer 字段**：每帧省一次重算，但要引入失效逻辑，并让 reducer 承担呈现关注点。分片不可变，`useMemo` 已给出同样的缓存效果。
- **按关注点拆分 `NodeState`**：分片形状改动会牵动全部写入路径，且与本次目标无关。派生层稳定之后再评。
- **只算完整视图，不设摘要级**：跨节点的运行中列表与命令面板要为每个 session 构造 transcript，代价随 session 数线性放大。
- **保留 `hub.selected` 并接上哈希**：两个所有者必然分叉。删掉只写不读的字段更简单。
- **为分片表遍历保留局部副本**：副本就是重复，改一处要记得改四处。

## 后果

- 派生用例直接构造 `(分片, sessionId)` 或 `(节点表, 分片表)` 输入，不经过 `reduce`；`reducer.test.ts` 只剩事实归约、事件去重、快照重放、断线续传与正文缓存。
- `sessionStateOf` 是 session 状态的唯一判定来源，侧栏摘要与主面板都调用它。
- 分支序号按 session 构造一次兄弟表，不再对每条消息过滤并排序整棵树；长 session 的渲染不再随消息数平方增长。
- 正文仍是可空语义：正文未拉到时 `TranscriptEntry.parts` 为 `null`，主面板渲染骨架，搜索与导出跳过该条。
- 新增呈现字段时先改派生层，再改组件；组件里不再出现「由分片现场推导」的代码。
- 协议新增字段时 `SessionTreeState` 自动获得该字段；分片装载函数不必改动。分片内部默认值的构造（空树）仍逐字段书写，新增字段在那处是编译错误，属于预期。
- 审查准则：`rg "Record<NodeName, NodeState>"` 的结果只应出现在 `derived.ts` 与 reducer 的写入路径上。
