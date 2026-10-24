# ADR 0040: 客户端多节点可用性

- 状态：已接受
- 日期：2026-10-25

## 背景

ADR 0024 之后客户端已经持有全部节点的分片状态，但交互仍按单节点设计：侧栏只按 workspace 展开列表，没有跨 session 搜索与快捷键，草稿只存在于组件状态，看不到其它节点在跑的 run，也没有未读标记。节点一多，这些缺口变成日常摩擦。规格只覆盖单节点视图，本 ADR 记录这次纯客户端补齐的边界与取舍。

## 决策

**命令面板是单一面板**。`Ctrl/⌘+K` 打开，分组为「session（跨节点）」「动作」「节点」。session 条目来自全部节点分片并按更新时间排序，选中后写 `#/n/<node>/s/<id>`。动作只列出当前可执行的项：注册节点始终可用；注册 workspace 要求选中节点 `ok`；停止 run 要求选中 session 正在运行。模糊匹配按子序列计分，连续命中与词首命中得分更高；`↑`/`↓`/`Enter`/`Esc` 可操作。面板状态留在 `App`，条目组装与过滤是 `view/command-palette.ts` 的纯函数。

**快捷键在输入框聚焦时让位**。`Ctrl/⌘+K`、`Esc`、`Alt+↑/↓`（切换 session）、`Alt+1…9`（切换节点）与 `?`（帮助）由 `view/shortcuts.ts` 解析。除 `Esc` 与命令面板外，目标为输入框、文本域或可编辑元素时一律让位给文本输入。`Esc` 在面板、帮助或对话框打开时只关闭当前层，不取消 run。节点序号按物理键位（`code`）识别，避免 macOS 的 `Alt+数字` 产生特殊字符。

**草稿按 (node, session) 存 `localStorage`**。键包含节点名与 session id；同时保存折叠的长粘贴块，刷新后可还原。空草稿删除键。每节点另存「上次打开的 session」，选中节点但没有 session 时恢复它；恢复只改 URL，不直接改组件状态。草稿在 `hashchange` 处理器与首次挂载时读取，避免在 effect 里同步派生状态。

**未读标记在分片内记录**。`NodeState` 增加 `lastSeenAt`，首次见到 session 时取当时的 `updatedAt`，之后只有更新时间前进才算未读。选中 session 时派发 `seen` 动作，时间只前进不回退。最近一次 run 成功或失败时，侧栏在未读标记上显示「完成」或「失败」。标记只在客户端内存中，刷新后重新以当前时间播种。

**正在运行面板从分片推导**。`view/activity.ts` 汇总全部节点里 `busy` 的 session、队列长度与当前 run 的开始时间。取消走既有的 `/api/n/<node>/sessions/<id>/cancel`。面板放在侧栏节点切换器下方。

**session 内搜索只查已加载正文**。命中按活动链顺序返回消息 id 与消息内序号，`Enter` 下一个、`Shift+Enter` 上一个，两端环绕。命中消息加 `data-message-id` 与 `data-search-hit`，滚动到可视区域并描边。不新增端点：活动链的正文已由正文拉取接口缓存。

**导出 Markdown 在客户端完成**。用户与助手正文按消息顺序输出，工具调用折叠为引用块并带结局，压缩消息渲染为引用块。下载用 `Blob` 与临时 `<a download>`，不经过服务端。

**输入历史与长粘贴只在 `Composer` 内处理**。`↑` 在光标位于行首时回溯当前 session 活动链上的用户消息，`↓` 回到草稿。超过 400 字符或 12 行的粘贴折叠为 `[粘贴内容 N：X 字符]`，原文随草稿保存，发送前还原。`Composer` 的 `onSend` 收到展开后的正文。

**页面标题反映当前 session**。`document.title` 形如 `执行中 · 标题 · 节点`；空闲时按最近一次 run 的结局加「失败」或「已取消」前缀，没有选中 session 时为 `durable`。

**不新增协议与端点**。全部数据来自既有分片、合并事件流与正文接口。为了让 `App` 不再增长，主视图抽到 `components/SessionPane.tsx`，网络接线抽到 `view/use-client-state.ts`；`scripts/source-budget.allow` 的 `App.tsx` 上限从 672 降到 593。

## 后果

- 新组件 `CommandPalette`、`ShortcutHelp`、`ActivityPanel`、`SearchBar` 各自有 Storybook stories，截图脚本自动覆盖浅色与深色。
- 搜索只覆盖活动链上已经加载的正文；未加载的兄弟分支不参与。
- 未读标记在刷新后清空，只用于当前页面会话期间区分「刚完成/刚失败」。
- 折叠粘贴的原文随草稿写入 `localStorage`；超长粘贴会让该键变大，但没有服务端成本。
- `Esc` 在搜索框聚焦时关闭搜索，不取消 run；取消 run 需要先关闭浮层。
- 纯客户端改动不改变 agent 与 hub 的协议，旧客户端与新客户端可以同时打开同一 hub。
