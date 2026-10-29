# client

React + Vite + TypeScript 客户端。设计 token 位于 `src/styles/tokens.css`，决策记录见 `../docs/adr/0009-design-tokens.md`。

## 命令

所有命令经 `devenv shell -- bash -c '...'` 执行。

| 命令 | 作用 |
| --- | --- |
| `npm run dev` | Vite 开发服务器，`/api` 代理到 `127.0.0.1:8081`（`agent`） |
| `npm run build` | 构建到 `client/dist` |
| `npm run lint` | ESLint（类型感知严格集，`--max-warnings=0`） |
| `npm run lint:css` | stylelint（语法、废弃关键字、颜色字面量） |
| `npm run typecheck` | `tsc --noEmit` |
| `npm run test` | Vitest（reducer、工具卡片、设计 token 与交互状态检查） |
| `npm run test:e2e` | Playwright 端到端测试（真实 agent 与浏览器） |
| `npm run storybook` | Storybook 开发服务器，端口 6006 |
| `npm run build-storybook` | 构建静态 Storybook 到 `client/storybook-static` |
| `npm run screenshots` | 从 `storybook-static` 生成截图 |

## host 可用性

| 能力 | 入口 |
| --- | --- |
| 命令面板 | `Ctrl/⌘+K`。session 按 host 分组，另有动作与 host 组；`↑`/`↓`/`Enter`/`Esc` 操作。 |
| 快捷键清单 | `?`。输入框聚焦时让位给文本输入，`Esc` 与命令面板除外。 |
| 切换 session | `Alt+↑` / `Alt+↓`，按侧栏顺序。 |
| 切换 host | `Alt+1…9`，按 host 面板顺序跳到该 host 最近的 session。 |
| 取消当前 run | `Esc`。浮层打开时先关闭浮层。 |
| 草稿 | 按 session id 存 `localStorage`；没有选中 session 时恢复上次打开的那一个。 |
| 正在运行 | 侧栏 host 面板下方的面板，按 host 分组列出在跑的 run 并可逐个停止。 |
| 未读标记 | 侧栏 session 右侧显示「完成」「失败」或未读点；只在当前页面会话期间有效。 |
| session 内搜索 | 页头放大镜按钮。`Enter` 下一个，`Shift+Enter` 上一个。 |
| 导出 Markdown | 页头下载按钮。工具调用折叠为引用块。 |
| 输入历史 | 输入框内 `↑` 回溯当前 session 已发送的消息，`↓` 回到草稿。 |
| 长粘贴折叠 | 超过 400 字符或 12 行时折叠为占位符，发送前还原。 |

这些能力全部在客户端完成。host 表、状态与事件都在 `agent` 的单一事件流里；URL 形如 `#/s/<session_id>`。

## 构建顺序

`agent` 二进制在编译期内嵌 `client/dist`。先构建 client，再构建 Rust：

```
devenv shell -- durable-build
```

## 端到端测试

```
devenv shell -- durable-build
devenv shell -- bash -c 'cd client && npx playwright install chromium'
devenv shell -- bash -c 'cd client && npm run test:e2e'
```

- `e2e/` 下的用例独享一套栈：脚本化 OpenAI 兼容 SSE 服务、真实 `agent`、临时 SQLite 与临时 workspace。页面由 `agent` 自己提供，没有 `hub`。
- `palette.spec.ts` 覆盖命令面板按 host 跳转与快捷键；`draft.spec.ts` 覆盖草稿隔离与上次 session 恢复；`reconnect.spec.ts` 覆盖 `agent` 重启后的重连与续传。
- 断言只用用户可见文本与可访问名称；前置数据经 `agent` 的 HTTP API 准备。
- 模型回答由 `e2e/support/model-server.ts` 的脚本队列决定。`hold` 脚本先流式返回文本再挂起，用于观察运行中的 run，测试调用 `release()` 收尾。
- 每个测试使用随机端口与临时目录，可并行执行。
- 失败时 `test-results/` 保存截图、trace 与页面快照；`npx playwright show-trace <trace.zip>` 可回放。
- 决策见 `../docs/adr/0025-browser-e2e.md`。

## Storybook

- `devenv shell -- storybook` 或 `npm run storybook`。
- 工具栏「主题」切换浅色与深色语义 token。
- 组件 stories 位于 `src/components/*.stories.tsx`；页面级 stories 为 `Pages/Console` 与 `Pages/Tokens`。
- host 状态由 `Components/HostSwitcher`、`Components/HostStatusBadge` 与 `Components/Sidebar` 覆盖；页面级 `Pages/Console` 另有 host 不可用、未知 session 与空 host 表三种状态。
- host 表在面板里维护：`注册 host` 打开注册对话框；每个 host 条目提供停用/启用、重连、重命名与删除。
- 组件 stories 通过 `parameters.frame` 选择画框：`sidebar`（280px，侧栏内组件）、`content`（748px，消息与工具行）、`centered`（自然尺寸居中，按钮与图标）、`page`（整页，shell 与页面）。画框只存在于 Storybook。

## 截图

```
npm run build-storybook
npm run screenshots
```

- 浅色输出：`client/screenshots/`。
- 深色输出：`client/screenshots/dark/`。
- 窄视口与短视口输出：`client/screenshots/viewports/`（1024×720 与 768×600，覆盖 `Pages/Console` 的全部 story）。
- 每张 1440×960，2 倍像素密度。
- 页面级 `Pages/Console` 的 host 状态 story：host 不可用与未知 session 没有输入区，断言标题与禁用命令；空 host 表断言切换器只有一个 `host` 占位。
- host 切换器断言：切换器存在、触发按钮不溢出切换器、选中时显示 host 名。
- 脚本逐张检查横向溢出与空文件；`Pages/Console` 中有输入区的 story 额外断言输入区与队列在 viewport 内（不需要滚动即可可见），任一张失败即以非零状态退出。
