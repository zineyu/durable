# ADR 0025: 浏览器端到端测试

- 状态：已接受
- 日期：2026-10-20

## 背景

ADR 0007 定义两个测试 seam：`agent` 的 HTTP API 与 client 纯 reducer。两者都替换了真实的浏览器与进程编排：渲染由 Storybook story 与 Chromatic 覆盖，网络与重连没有 seam。

这留下一条没有人覆盖的缝隙：客户端从事实事件推导视图。reducer 测试的输入是手写事件序列，story 的输入是完整 fixture；两者都不会发现「服务端漏发事实、客户端因此不更新」这类接线错误。

ADR 实施期间，浏览器端到端测试立即发现了一个这样的缺陷：`start_run` 与 `edit_user_message` 移动 active leaf 时只写数据库、不发事实，已连接的浏览器因此看不到新消息，直到刷新或重连。

## 决策

新增 **Seam 4：浏览器端到端**。`Playwright` 启动真实进程：

- 脚本化 OpenAI 兼容 SSE 服务（真实本地 HTTP 服务，不替换被测代码）；
- 真实 `agent` 二进制，临时 SQLite 与临时 workspace；
- 真实 `hub` 二进制，临时 SQLite，监听随机回环端口；
- Chromium 加载 `hub` 托管的客户端。

每个测试独享一套栈：临时目录、随机端口、独立脚本队列。测试用 `hub` 的公开 HTTP API 做前置准备（注册节点、创建 session），其余操作经浏览器完成：注册 workspace、创建 session、提交消息、取消、编辑、重试、切换分支、移除队列项。断言只用用户可见的文本与可访问名称。

脚本队列按模型请求顺序消费。脚本种类为纯文本、工具调用、先流式返回再挂起（用于观察运行中的 run）、非成功状态。挂起脚本由测试显式释放。

用例位于 `client/e2e/`，执行入口为 `devenv shell -- durable-e2e`。它不属于 `durable-test`：它需要构建产物与 Playwright 浏览器，且比单元测试慢。

## 后果

- 补上「浏览器 + 真实二进制 + 真实事件流」这一层。断线重连、`hub` 重启、SSE 续传首次进入自动化覆盖。
- 断言依赖可访问名称，因此组件必须保持可访问性；为此为输入区补了 `aria-label`。
- 测试要求先构建 client 与 Rust workspace。缺少二进制时用例显式失败并提示 `durable-build`。
- CI 增加 `npx playwright install --with-deps chromium` 与端到端步骤。
- 视觉回归仍由 Storybook 与 Chromatic 负责；端到端用例不做像素断言。
