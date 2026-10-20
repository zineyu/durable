# client

React + Vite + TypeScript 客户端。设计 token 位于 `src/styles/tokens.css`，决策记录见 `../docs/adr/0009-design-tokens.md`。

## 命令

所有命令经 `devenv shell -- bash -c '...'` 执行。

| 命令 | 作用 |
| --- | --- |
| `npm run dev` | Vite 开发服务器，`/api` 代理到 `127.0.0.1:8080`（`hub`） |
| `npm run build` | 构建到 `client/dist` |
| `npm run lint` | ESLint |
| `npm run typecheck` | `tsc --noEmit` |
| `npm run test` | Vitest（reducer、工具卡片、设计 token 与交互状态检查） |
| `npm run storybook` | Storybook 开发服务器，端口 6006 |
| `npm run build-storybook` | 构建静态 Storybook 到 `client/storybook-static` |
| `npm run screenshots` | 从 `storybook-static` 生成截图 |

## 构建顺序

`hub` 二进制在编译期内嵌 `client/dist`。先构建 client，再构建 Rust：

```
devenv shell -- durable-build
```

## Storybook

- `devenv shell -- storybook` 或 `npm run storybook`。
- 工具栏「主题」切换浅色与深色语义 token。
- 组件 stories 位于 `src/components/*.stories.tsx`；页面级 stories 为 `Pages/Console` 与 `Pages/Tokens`。
- 节点状态由 `Components/NodeSwitcher`、`Components/NodeStatusBadge` 与 `Components/Sidebar` 覆盖；页面级 `Pages/Console` 另有不可达节点、未知节点与空节点表三种状态。
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
- 页面级 `Pages/Console` 的节点状态 story：不可达节点与未知节点没有输入区，断言标题与禁用命令；空节点表断言切换器没有节点。
- 节点切换器断言：切换器存在、节点按钮不溢出切换器、选中数量符合预期。
- 脚本逐张检查横向溢出与空文件；`Pages/Console` 中有输入区的 story 额外断言输入区与队列在 viewport 内（不需要滚动即可可见），任一张失败即以非零状态退出。
