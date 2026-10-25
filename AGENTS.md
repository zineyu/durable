# durable

本机单用户的 coding agent：`agent` 只提供 HTTP API，`hub` 托管内嵌的 React 客户端并把 `/api/*` 原样反代给 `agent`。

- 规格：`docs/specs/rust-react-coding-agent.md`（主 issue 的正文即该规格）。
- 架构决策：`docs/adr/`。
- 领域术语：`docs/glossary.md`。代码、提交说明与文档使用术语表词汇。
- 客户端命令、Storybook 画框与截图：`client/README.md`。

## 目录

| 路径 | 职责 |
| --- | --- |
| `crates/core` | 领域与持久化内核（包名 `durable`）；领域类型、`ts-rs` 派生、单一写入线与 SQLite 存储 |
| `crates/runtime` | 进程级支撑（包名 `runtime`）；平台标准目录解析与日志初始化 |
| `crates/agent` | agent 二进制；HTTP/SSE 协议与 `ts-rs` 导出入口 |
| `crates/hub` | hub 二进制；编译期内嵌 `client/dist` |
| `client` | React + Vite + TypeScript 客户端 |
| `docs` | 规格、ADR（索引见 `docs/adr/README.md`）、术语表 |
| `scripts` | 门禁与检查脚本；编辑时跑 `bash scripts/lint-fast.sh` |

## crate 导航

`core`（包名 `durable`）：

| 模块 | 职责 |
| --- | --- |
| `id.rs` | 强类型 id，`WorkspacePath` 与 `SessionTitle` 的解析校验 |
| `message.rs` | 消息正文、角色与 `MessageNode` 的父子指针 |
| `session.rs` | session 元数据、`SessionTree` 快照与自动标题 |
| `queue.rs` | 待执行队列项与锚点 |
| `run.rs` | 运行实体、状态与失败码 |
| `tool.rs` | 工具调用记录、结果与错误码 |
| `workspace.rs` | workspace 元数据 |
| `node.rs` | 节点名与节点元数据 |
| `event.rs` | `FactEvent` 领域事实枚举 |
| `change.rs` | 一次事务内的变更清单 |
| `stream.rs` | `StreamEvent` 与进程内广播 |
| `store/` | 唯一写入线：`mod.rs` 门面与提交入口、其余子模块按对象分写入命令 |
| `storage/` | 快照查询与全部 SQL |

`runtime`：`dirs.rs` 跨平台标准目录与启动路径；`logging.rs` 日志双写与轮转。

`agent`：

| 模块 | 职责 |
| --- | --- |
| `api.rs` | HTTP 路由与处理器 |
| `engine/` | run 引擎：`mod.rs` 执行循环与收尾、`step.rs` 模型流与重试、`tools.rs` 工具循环、`prompt.rs` 请求装配、`compact.rs` 上下文压缩、`recover.rs` 续跑推导 |
| `provider.rs` | OpenAI 兼容 provider 与 SSE 解析 |
| `tools/` | 工具注册表与四个工具；`paths.rs` 管 workspace 边界 |
| `sse.rs` | 单一全局事件流：回放、实时与 resync |
| `protocol.rs` | 对外协议 DTO 与协议版本 |
| `config.rs`、`error.rs`、`cancel.rs` | 配置解析、HTTP 错误信封、取消信号 |

`hub`：`lib.rs` 路由、节点作用域转发与静态资源；`nodes.rs` 节点表命令与节点快照；`bus/` 广播序号、回放窗口、节点视图、进程内计数与上游 watcher；`sse.rs` 合并事件流；`node_probe.rs` 上游探测；`protocol.rs` 与 `error.rs`。

`client/src`：`api/` 协议客户端与生成类型；`view/` 纯 reducer 与状态推导；`components/`、`stories/`、`styles/`。

## 模块头

`crates/*/src` 下每个模块的第一行是 `//!`，写「本模块负责什么」与「不属于它的在哪」。新增模块必须带这一行；移动职责时同步改，否则下一轮会照着过期的说明改错文件。

## 任务 → 文件

| 我要改… | 从这里开始 |
| --- | --- |
| agent 的 HTTP 接口 | `crates/agent/src/api.rs` 的路由表与处理器，DTO 在 `protocol.rs` |
| SSE 事件载荷 | `crates/agent/src/sse.rs`（传输）与 `crates/core/src/stream.rs`（类型） |
| 新的领域事实 | `crates/core/src/event.rs`，写入路径在 `store/` 与 `change.rs` |
| 新的 LLM 工具 | `crates/agent/src/tools/` 新文件，并在 `tools/mod.rs` 的 `ToolBox` 实现里登记 `definitions()` 与 `execute()` |
| 表结构或迁移 | `crates/core/migrations/` 新增文件，SQL 在 `crates/core/src/storage/sqlite/mod.rs` 与同级的 `apply.rs` |
| hub 转发与多节点行为 | `crates/hub/src/lib.rs` 的 `forward_node` 与 `bus/watch.rs` |
| 节点表命令或节点快照 | `crates/hub/src/nodes.rs`，路由在 `lib.rs` |
| 协议类型与 TS 生成 | `crates/agent/src/lib.rs`、`crates/hub/src/lib.rs` 的 `export_types`，再跑 `durable-typegen` |
| 客户端状态推导 | `client/src/view/reducer.ts` 与 `node-state.ts` |
| 样式 token 与组件样式 | `client/src/styles/tokens.css` 与组件同名 `*.module.css` |
| 集成测试 | `crates/*/tests/`，脚手架在 `tests/support/` |
| 浏览器端到端测试 | `client/e2e/` |

## 命令

所有命令经 `devenv shell --` 执行。首次使用先安装客户端依赖：`devenv shell -- bash -c 'cd client && npm ci'`。

| 命令 | 作用 |
| --- | --- |
| `devenv shell -- durable-build` | 先构建 client，再构建 Rust workspace |
| `devenv shell -- durable-lint` | 全部静态检查，不含测试 |
| `devenv shell -- durable-lint-fast` | 只含秒级静态检查，编辑时反复运行 |
| `devenv shell -- durable-test` | 提交前的全部检查：lint 与测试 |
| `devenv shell -- durable-e2e` | 端到端测试：构建后由 Playwright 驱动真实 agent、hub 与浏览器 |
| `devenv shell -- durable-typegen` | 重新生成 TS 协议类型 |
| `devenv shell -- storybook` | Storybook 开发服务器 |

`durable-test` 先跑 `durable-lint`，再跑 `cargo test --workspace`、Vitest 与 TS 类型一致性比对。提交前必须全部通过。

门禁本体是版本库里的两条脚本，`devenv.nix` 只负责转调：

- `scripts/lint-fast.sh`：`cargo fmt --all --check`、`cargo check --workspace --all-targets`、ESLint（`--max-warnings=0`）、stylelint、`tsc --noEmit`。
- `scripts/lint.sh`：先跑快 lane，再加 `cargo clippy --workspace --all-targets -- -D warnings`、`RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --lib --document-private-items`、`cargo machete`、`cargo deny check`、`typos`、`taplo fmt --check`、`actionlint`、`shellcheck scripts/*.sh`、`nixfmt --check devenv.nix`，最后跑 `scripts/check-*.sh`。

规则与理由见 `docs/adr/0032-strict-lint-gate.md`。

## 检查门禁

所有告警与错误都阻断。代码里不允许出现抑制：

- Rust 用 `#[expect(<lint>, reason = "…")]`，不用 `#[allow]`。`#[expect]` 在不触发时自身报错，因此过时抑制会被发现。
- 生产代码（`crates/*/src/`）不出现 `#[expect]`。环境所限的最小例外已在 ADR-0032 列出。
- 测试脚手架在文件头用一条 `#![expect(…, reason = "…")]`，不逐行散放。
- TS 侧 `reportUnusedDisableDirectives` 为错误；新代码不应需要 `eslint-disable`。

`durable-e2e` 不包含在 `durable-test` 内：它需要构建产物与 Playwright 浏览器，首次使用先运行 `devenv shell -- bash -c 'cd client && npx playwright install chromium'`。涉及浏览器行为、SSE 断线重连或 hub 重启的改动必须运行它。见 `docs/adr/0025-browser-e2e.md`。

## 规模与依赖预算

- 规模预算：`crates/*/src` 与 `crates/*/tests` 下单个 Rust 文件 600 行，`client/src` 下单个 TS/TSX 文件 400 行（不含 `*.test.ts(x)` 与 `src/api/generated/`）。超限必须拆文件；存量欠账登记在 `scripts/source-budget.allow`，登记值只许变小。
- 依赖白名单：每个包的直接依赖（含 dev 依赖）都登记在 `deps.allow`，新增依赖必须先写明理由。

两条检查由 `scripts/check-source-budget.sh` 与 `scripts/check-deps.sh` 执行，已包含在 `durable-lint` 内。

门禁本身不写在检查脚本里：`.github/workflows/gate-watch.yml` 在 PR 上比对门禁文件与 `.github/` 的改动，命中时评论提醒审查者，不阻断合并。

## 契约一致性

- 规格追溯：代码与测试里的 `规格 user story A-16`（`crates/hub` 写 `H-16`）必须对应 `docs/specs/` 里存在的编号，且前缀与所在目录匹配；不属于规格的决定写 `ADR 00NN：…`。未被引用的编号只打印清单，不阻断。脚本 `scripts/check-spec-links.sh`。
- 类型清单：`docs/api/types.md` 与 TS 类型同一来源、同一命令生成，`durable-test` 比对两者，见「生成物」。
- 术语表：`docs/glossary.md` 里标识符形状的反引号术语必须能在 `crates/` 或 `client/` 里找到，否则视为过时。脚本 `scripts/check-glossary.sh`。

## 日志与路径

`--database` 与 `--log-dir` 可省略：省略时使用平台标准目录（见 `docs/adr/0026-logging-and-default-paths.md`）。日志同时写入标准错误（pretty 格式）与每日轮转的 JSON Lines 文件，保留 14 天。`DURABLE_LOG` 控制过滤，默认 `info`。测试必须传 `--log-dir`，避免写入用户的标准日志目录。

## 构建顺序

`hub` 在编译期读取 `client/dist`，缺失或过期时行为不可预期。任何 Rust 构建之前先构建 client；`durable-build` 已按此顺序执行。

## 生成物

`client/src/api/generated/` 与 `docs/api/types.md` 由 `ts-rs` 导出生成，不手写。Rust 协议类型变更后运行 `devenv shell -- durable-typegen`，并把生成文件的变更一并提交。`durable-typegen` 的实现是 `scripts/typegen.sh`，`durable-test` 会重新生成后比对这两个路径。

## 前端约定

- 组件只消费 `client/src/styles/tokens.css` 的语义 token，不写颜色字面量与硬编码长度。
- 页面不含注释性说明。
- 组件样式由确定性检查与截图脚本断言：见 `docs/adr/0010-client-style-verification.md`。
- ESLint 开启类型感知严格集：禁止 `any`、非空断言与未处理联合分支。

## 工作区噪声

工作区包含 devenv 与构建产物目录：`.devenv/`、`target/`、`client/dist/`、`client/storybook-static/`、`client/screenshots/`、`client/test-results/`、`client/playwright-report/`。扫描仓库使用 `rg` 或 `git ls-files`，不要用 `find .`。
