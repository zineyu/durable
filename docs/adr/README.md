# ADR 索引

该表由各篇 ADR 的「决策」段压缩而来，细节看各篇原文。

| 编号 | 标题 | 状态 | 结论 |
| --- | --- | --- | --- |
| [0001](0001-two-binaries.md) | C/S 分离为 agent 与 ui 两个二进制 | 已接受 | 产出 agent 与 ui 两个二进制，由 ui 托管前端并反代 API。 |
| [0002](0002-sse-protocol.md) | HTTP 命令 + 全局单一 SSE 事件流 | 已接受 | 命令走 HTTP JSON，事件走单一全局 SSE 流，按 after 游标续传。 |
| [0003](0003-multi-workspace-sessions.md) | 多 workspace、多 session 与持久 FIFO 队列 | 已接受。并发槽位部分由 ADR 0023 取代 | session 绑定唯一 workspace，每个 session 有持久 FIFO 队列。 |
| [0004](0004-message-tree.md) | 消息树与分支历史 | 已接受 | session 消息组织为树并保存 active leaf，分支切换只移动 active leaf。 |
| [0005](0005-no-approval-tools.md) | 无审批、限定 workspace 根的工具执行 | 已接受 | 四个工具全自动执行，不设审批，路径限定在 workspace 根内。 |
| [0006](0006-openai-provider.md) | 单一 OpenAI 兼容 provider 与显式上下文失败 | 已接受 | 只实现 OpenAI 兼容流式 provider，超上下文上限时 run 以明确错误结束。 |
| [0007](0007-test-seams.md) | 测试 seam：agent HTTP API 与 client 纯 reducer | 已接受。seam 清单由 ADR 0046 增加 seam 五 | 设两个测试 seam：真实 agent HTTP API 与 client 纯 reducer。 |
| [0008](0008-typegen-ci.md) | 生成式协议类型与 CI 门禁 | 已接受 | 用 ts-rs 生成 TS 类型并提交，CI 重新生成后 diff 必须为空。 |
| [0009](0009-design-tokens.md) | 设计 token 采用 DeepSeek Harness 的两层体系 | 已接受 | 设计 token 分原始色阶与语义别名两层，组件只引用语义 token。 |
| [0010](0010-client-style-verification.md) | 前端样式不变量用确定性检查覆盖 | 已接受 | 样式不变量由确定性测试与截图脚本断言覆盖，不依赖 Chromatic。 |
| [0011](0011-metadata-conventions.md) | workspace 与 session 元数据的持久化约定 | 已接受 | 规范化绝对路径作 workspace 身份，删除 session 保留一条删除事实事件。 |
| [0012](0012-durable-kernel.md) | durable 内核与持久化边界 | 已接受 | 新增 core crate 承载内核，单一写入线用 Changeset 串行化事务。 |
| [0013](0013-queue-and-auto-title.md) | 队列项与自动标题 | 已接受 | 队列项为独立领域对象，准入只入队，标题按首条消息自动更新。 |
| [0014](0014-run-engine.md) | run 引擎与事件流 | 已接受。并发槽位与 `DURABLE_MAX_CONCURRENCY` 部分由 ADR 0023 取代 | run 为持久实体，一次 run 产生一个 assistant 节点并节流落库文本。 |
| [0015](0015-tool-execution.md) | 工具执行与 workspace 路径边界 | 已接受 | ToolRegistry 实现四个工具，路径限定根内，错误码为稳定枚举。 |
| [0016](0016-branch-commands.md) | 分支命令与 active leaf 切换 | 已接受 | 队列项可锚定用户节点，编辑与重试复用 run 引擎，切换只改 active leaf。 |
| [0017](0017-client-state-derivation.md) | 客户端状态推导与事件流接线 | 已接受 | 单一纯 reducer 从事件流与快照推导全部客户端状态，组件只收 props。 |
| [0018](0018-sse-resync.md) | SSE 落后检测与快照回放恢复 | 已接受。客户端游标部分由 ADR 0024 取代 | 落后时发 Resync 事件，快照带水位，落地快照时回退游标并重开事件流。 |
| [0019](0019-tool-call-facts.md) | 工具调用以事实事件增量广播 | 已接受 | 每个工具执行完成后追加 tool_call_recorded 事实事件。 |
| [0020](0020-chromatic-optional.md) | Chromatic 门禁按 token 存在启用 | 已接受 | Chromatic 步骤仅在存在项目 token 时执行，无 token 时跳过不阻塞。 |
| [0021](0021-hub-as-multi-node-bus.md) | hub 作为多节点总线 | 已取代。见 ADR 0048 | ui 改名 hub 并升级为总线：持有节点表、合并协议、广播事实事件。 |
| [0022](0022-node-name-as-identity.md) | 节点名即身份 | 已取代。见 ADR 0048 | 节点名即节点身份，全局 session 键为节点名加 session id。 |
| [0023](0023-remove-concurrency-slots.md) | 移除并发槽位 | 已接受。ADR 0048 未改变并发决定 | 删除全局并发槽位与相关配置，取出队首即启动 run。 |
| [0024](0024-client-node-shards.md) | 客户端分片状态与广播游标 | 已取代。见 ADR 0048 | 客户端状态按节点分片，广播游标与分片水位分开，快照在分片内补尾部。 |
| [0025](0025-browser-e2e.md) | 浏览器端到端测试 | 已接受 | 新增 Seam 4 浏览器端到端测试，用真实进程栈加 Playwright 驱动。 |
| [0026](0026-logging-and-default-paths.md) | 日志与默认路径 | 已接受 | 数据库与日志目录均可省略，日志同时输出标准错误与按天轮转文件。 |
| [0027](0027-upstream-subscription-starts-at-node-watermark.md) | 上游订阅起点取节点快照水位 | 已接受。细化 ADR 0021 | 上游订阅起点取该节点快照水位，不持久化上游游标。 |
| [0028](0028-forwarding-timeouts-and-status-ownership.md) | 节点作用域转发的超时与状态归属 | 已接受。管理视图写入由 ADR 0041 细化 | 转发失败不改节点状态，超时按响应类型分档而不按路径判定。 |
| [0029](0029-bootstrap-watermark-before-node-requests.md) | 合并 bootstrap 的广播水位在任何节点请求之前取 | 已接受。细化 ADR 0018 的水位定义与 ADR 0021 的合并 bootstrap | 合并 bootstrap 在任何节点请求前取广播水位，各节点请求并发发起。 |
| [0030](0030-upstream-resync-is-reconnect-not-failure.md) | 上游 resync 只触发重连，不写节点状态 | 已接受。细化 ADR 0018 的 resync 语义与 ADR 0021 的节点状态机 | 上游 Resync 只结束订阅并按原游标重连，不写节点状态。 |
| [0031](0031-provider-rate-limit-retry.md) | 供应商限流用独立失败类别并有限重试 | 已接受。细化 ADR 0014 的失败类别与 ADR 0018 的实时事件 | 新增 model_rate_limited 类别，在单次模型调用内有限重试，只发实时提示。 |
| [0032](0032-strict-lint-gate.md) | 严格 lint 门禁 | 已接受 | durable-lint 统一跑十类检查，抑制须带理由且失效即报错。 |
| [0033](0033-gate-second-layer-and-navigation.md) | 门禁第二层：规模、依赖、PR 自锁与仓库导航 | 已接受 | 门禁进版本库并分快慢两 lane，加规模与依赖预算、PR 自锁提醒和文档一致性检查。 |
| [0034](0034-spec-number-prefixes.md) | 规格编号加前缀并按目录归属校验 | 已接受。细化 ADR 0033 的契约一致性 | 两份规格分别用 A- 与 H- 前缀，引用按目录归属校验，非规格决定改引 ADR。 |
| [0035](0035-message-content-blocks.md) | 消息历史存完整内容，节点正文为内容块数组 | 已接受。取代 ADR 0004 中节点正文的形状 | 节点是一条消息，正文为有序内容块；工具结果独立成节点；落库为 message 与 message_part 两表。 |
| [0036](0036-compaction-message.md) | 压缩消息在链上，装配回溯到它为止 | 已接受 | 压缩是链上的 `compaction` 节点，装配从 active leaf 回溯到它即停，压缩不属于任何 run。 |
| [0037](0037-crash-recovery.md) | 崩溃恢复的续跑点从树推导 | 已接受。取代 ADR 0014 的节流落库部分输出；「assistant 消息只在定型时入树」由 ADR 0047 取代 | tool 节点先入树再执行，续跑点由树尾推导，非幂等工具崩后结局记为未知。 |
| [0038](0038-skeleton-events.md) | 事件与快照只带骨架，正文按需拉取 | 已接受。取代 ADR 0019 的 `tool_call_recorded` | 事实事件与快照只带节点骨架，正文按消息 id 批量拉取并缓存，进行中的内容挂在未来定型的消息 id 上。 |
| [0039](0039-steer-at-step-boundary.md) | 插话在步骤边界接入当前 run | 已接受 | `mode=steer` 的插话在提交时入树，在模型请求与收尾前的步骤边界接入同一个 run；工具不打断；run 结束未消费则降级为排队。 |
| [0040](0040-client-multi-node-usability.md) | 客户端多节点可用性 | 已接受 | 命令面板、快捷键、按 (node, session) 的草稿、跨节点运行面板、未读标记、session 内搜索与 Markdown 导出都在客户端完成，不新增协议。 |
| [0041](0041-node-table-commands-write-node-view.md) | 节点表命令可以写节点视图 | 已接受。细化 ADR 0028 的状态归属 | 探测结果只由 watcher 写，停用与等待连接的管理状态由节点表命令写，共用同一条 node_status_changed。 |
| [0042](0042-fact-envelope-carries-session-view.md) | 事实信封携带 session 视图 | 已接受。取代 ADR 0013 的队列长度递减约定，细化 ADR 0012 的单一写入线 | 事实载荷只留身份，session 视图由命令入口推导并落在事件信封上，`event` 表新增 `session` 列，协议版本升到 5。 |
| [0043](0043-store-error-classification.md) | 错误分类归内核 | 已接受。细化 ADR 0014 的失败类别、ADR 0028 的状态码选型与 ADR 0031 的类别约定 | `StoreError` 给出无载荷的分类与结局类别，状态码只由结局类别决定，两个 seam 穷尽映射协议码并把节点码统一为 `node_not_found`；新增 `storage_failed` 失败类别，协议版本升到 6。 |
| [0044](0044-client-derived-view.md) | 客户端派生视图层 | 已接受。细化 ADR 0017 的「组件只接收派生后的 props」与 ADR 0024 的分片形状 | 新增纯函数派生层 `view/derived.ts`，摘要全算、完整视图按需算，按分片对象缓存；它是分片表的唯一常规读取者，组件只收 `SessionView`；`SessionTreeState` 由 `SessionTreeSnapshot` 派生；选中位置只由 URL 哈希持有。 |
| [0045](0045-message-tree-module.md) | 消息树成为独立 module | 已接受。细化 ADR 0036 的压缩截断与 ADR 0038 的骨架读取 | 新增领域类型 `MessageTree`，一次建索引后只回答树的问题；存储只返回行，链上顺序、压缩截断与叶子选择由它回答；协议快照改名 `SessionTreeSnapshot`。 |
| [0046](0046-store-test-seam.md) | 第三条测试 seam：`Store` | 已接受。补充 ADR 0007 的 seam 清单 | 新增内核层 seam：用临时 SQLite 打开真实 `Store`，直接断言快照、事件序列与错误变体；协议、SSE、进程与工具仍由 Seam 1 覆盖。 |
| [0047](0047-stream-progress-persistence.md) | 流式进度落库与已中断消息 | 已接受。取代 ADR 0037 的「assistant 消息只在定型时入树」 | 流式正文按时间节流写入 run 的进度，崩溃恢复与取消时入树为标注已中断的 assistant 消息，续跑接着它继续。 |
| [0048](0048-remote-ssh-shaped-process-model.md) | 进程模型反转为 remote-ssh 形状 | 已接受。取代 ADR 0001 的二进制划分与 ADR 0021 至 0024 的总线与分片决策 | `agent` 独占 loop 与 SQLite，工具执行交给 host 上的 executor，两侧经 SSH 上的 JSON 帧通信；状态集中，客户端回到单游标。 |
