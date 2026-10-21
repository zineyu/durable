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
| `docs` | 规格、ADR、术语表 |

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

## 日志与路径

`--database` 与 `--log-dir` 可省略：省略时使用平台标准目录（见 `docs/adr/0026-logging-and-default-paths.md`）。日志同时写入标准错误（pretty 格式）与每日轮转的 JSON Lines 文件，保留 14 天。`DURABLE_LOG` 控制过滤，默认 `info`。测试必须传 `--log-dir`，避免写入用户的标准日志目录。

## 构建顺序

`hub` 在编译期读取 `client/dist`，缺失或过期时行为不可预期。任何 Rust 构建之前先构建 client；`durable-build` 已按此顺序执行。

## 生成物

`client/src/api/generated/` 由 agent 的 `ts-rs` 导出生成，不手写。Rust 协议类型变更后运行 `devenv shell -- durable-typegen`，并把生成文件的变更一并提交。

## 前端约定

- 组件只消费 `client/src/styles/tokens.css` 的语义 token，不写颜色字面量与硬编码长度。
- 页面不含注释性说明。
- 组件样式由确定性检查与截图脚本断言：见 `docs/adr/0010-client-style-verification.md`。
- ESLint 开启类型感知严格集：禁止 `any`、非空断言与未处理联合分支。

## 工作区噪声

工作区包含 devenv 与构建产物目录：`.devenv/`、`target/`、`client/dist/`、`client/storybook-static/`、`client/screenshots/`、`client/test-results/`、`client/playwright-report/`。扫描仓库使用 `rg` 或 `git ls-files`，不要用 `find .`。
