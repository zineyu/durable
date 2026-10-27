# ADR 0007: 测试 seam：agent HTTP API 与 client 纯 reducer

- 状态：已接受。seam 清单由 ADR 0046 增加 seam 五
- 日期：2026-02-14

## 背景

测试需要确定性，且不得用 mock 替换被测代码。

## 决策

seam 一：真实 agent 进程 + 临时 workspace + 临时 SQLite，指向脚本化 OpenAI 兼容 SSE 服务，经 HTTP 驱动。seam 二：client 纯事件 reducer 与 fixture props，用 Vitest 覆盖。渲染状态由 Storybook stories 与 Chromatic 覆盖。seam 五（ADR 0046）：真实 `Store` 与临时 SQLite，经公开写命令与读方法直接断言内核行为。seam 三由多节点规格定义，seam 四由 ADR 0025 定义。

## 后果

agent 循环、工具、队列、分支、持久化、回放均通过公开 API 验证。真实供应商行为差异与浏览器网络行为不在测试范围内。
