# ADR 0010: 前端样式不变量用确定性检查覆盖

- 状态：已接受
- 日期：2026-10-19

## 背景

设计 token 与组件样式需要可审查的门禁。Chromatic 只能回答「渲染结果是否变化」，不能回答「颜色是否只来自语义 token」「对比度是否达标」「布局不变量是否保持」，而且它依赖仓库 secret。没有确定性检查时，样式回归只能靠人工查看截图。

## 决策

样式不变量由确定性检查覆盖，作为 `durable-test` 与截图脚本的一部分：

- `design-tokens.test.ts`：组件 `*.module.css` 不含颜色字面量，不含硬编码长度值（`@` 条件行除外）；`tokens.css` 提供颜色、字体、间距三类 token；`global.css` 定义 `:focus-visible`。
- `contrast.test.ts`：解析 `tokens.css` 的浅色与深色语义别名并解析 `var()` 引用，正文文本对每个背景 token 的对比度不低于 4.5:1。
- `interaction-states.test.ts`：出现 `:hover` 的组件样式同时定义 `:active`。
- `screenshot.mjs`：逐张截图时断言无横向溢出、截图为非空文件、页头标题与输入卡片左边缘对齐、消息列相对输入区内缩 16px、分支翻页组件贴右且在消息底部、session 操作菜单 hover 后展开且不少于两项；`Pages/Console` 额外断言输入区与队列在 viewport 内。节点状态页的标题断言取 `main h2`，避免侧栏新增分组标题干扰。

截图脚本先清空输出目录再写入，失败即非零退出。

## 后果

token、对比度与布局不变量回归在本地 `durable-test` 或截图脚本中立即失败，不依赖 Chromatic 是否可用。Chromatic 仍是合并门禁，覆盖这些断言之外的视觉差异。

对齐与贴边断言的容差为 1px，与 0.5px 发丝描边和 2 倍像素密度截图一致；断言阈值随设计决策调整，不改动组件即可更新。
