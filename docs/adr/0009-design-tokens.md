# ADR 0009: 设计 token 采用 DeepSeek Harness 的两层体系

- 状态：已接受
- 日期：2026-02-14

## 背景

前端需要一套可审查、可复用的视觉基线。DeepSeek Harness（`deepseek-ai/deepseek-harness`）公开了它的主题实现：`packages/client/ui-theme/src/styles/` 中的 `base.css` 与 `design-platform.css` 定义静态色阶和语义别名，`docs/web-styling.md` 与 `docs/ui-radius.md` 规定组件只消费语义 token、0.5px 发丝描边、按组件高度选取圆角、高层级表面使用带描边的阴影。

## 决策

采用同一套两层结构：

- 原始色阶：`--palette-neutral-bluish-*`、`--palette-deepseek-*`、`--palette-red/amber/green-*`、`--palette-blue-500`。取值与 DeepSeek Harness 的 `--dsw-static-*` 一致。
- 语义别名：`--color-bg`、`--color-surface-*`、`--color-text*`、`--color-border-*`、`--color-accent*`、`--color-primary-*`、`--color-success/warn/danger*`，以及 `--elevation-*`、`--radius-*`、`--text-*`、`--leading-*`、`--space-*`、`--duration-*`、`--ease-standard`、布局尺寸。
- 组件只引用语义 token，不写颜色字面量，不引入本地圆角值与间距值。
- 主按钮使用近黑填充（`--color-primary-fill`），强调色单独使用 DeepSeek 蓝（`--color-accent`），用于状态点、焦点环与选中态。
- 圆角按组件角色：2 / 4 / 8 / 12 / 16 / 20。侧栏 session 行、按钮、工具行用 4；代码块、队列面板、空状态卡片用 12；用户气泡用 16；输入卡片用 20；菜单项与徐标用 2。相对 DeepSeek Harness 的 4 / 8 / 12 / 16 / 20 / 28 整体下调一档，以更紧凑的轮廓匹配本工具的密度。
- 侧栏选中行使用 `--color-accent-soft` 填充与 `--color-accent-text` 标题，不使用左侧色条；session 操作在行尾用单个图标按钮展开图标菜单，hover、focus-within 与选中时可见，标题溢出时 hover 横向滚动。
- 视觉按「亲密、对齐、重复、对比」四原则组织：状态与说明同组；页头、队列、输入区共用 `--composer-max-width` 列宽，transcript 保持窄 32px；状态点由 `StatusDot` 统一，图标按钮由 `IconButton` 统一；工具状态与消息状态使用语义色文字而非同级灰。
- 中性实线描边使用 0.5px 发丝线。高层级表面（输入卡片）使用 `--elevation-soft` 且 `border: 0`，其第一层是发丝描边（`gradient-shadow-text.css` 的 `--dsw-elevation-soft`）；与输入卡片相接的队列面板改用 0.5px 发丝描边，与参考实现的 `ui-conversation/src/client/queue/QueueDock.module.css` 中 `.panel::after` 的做法一致，避免在接缝处多画一条投影描边。
- 组件不引入本地颜色值与本地圆角值；间距取 `--space-*`，组件高度取 `--control-height-*`，装饰尺寸取 `--dot-size`、`--marker-size`、`--caret-height`。
- 字体使用系统栈与等宽栈，不加载外部字体。
- 同时定义浅色与深色语义别名；产品默认浅色，Storybook 提供主题切换用于审查。

## 后果

- 组件样式集中在语义层，替换主题只需改语义别名。
- Storybook 用 `parameters.frame` 限定组件画框（`sidebar` 280px、`content` 748px、`centered`、`page`），组件不在整页宽度下拉伸；该画框只存在于 Storybook。
- 侧栏 session 行使用三列网格：状态点、标题与状态、时间；行尾另留 24px 操作槽位（20px 按钮 + 4px 内边距），时间右边缘距行右边缘 24px，各行一致。操作按钮在 hover、focus-within 与选中时出现，垂直居中于行内，不遮挡时间。
- 分支翻页组件位于 assistant 消息底部右侧，无边框，仅用 hover 与 active 背景表示交互。

- 组件样式集中在语义层，替换主题只需改语义别名。
- 对比度按语义层校验：正文 18.9:1，次要文字 5.8:1，三级文字 4.7:1，强调文字 5.39:1；深色主题各档不低于 7.8:1。
- 深色别名在 Storybook 与截图脚本中覆盖，Chromatic 默认快照浅色。
- 设计 token 的来源与取值可在本仓库外核对：`packages/client/ui-theme/src/styles/base.css`、`design-platform.css`、`gradient-shadow-text.css`，以及 `docs/web-styling.md`、`docs/ui-radius.md`。
