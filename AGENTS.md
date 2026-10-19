# durable

本机单用户的 coding agent：`agent` 只提供 HTTP API，`ui` 托管内嵌的 React 客户端并把 `/api/*` 原样反代给 `agent`。

- 规格：`docs/specs/rust-react-coding-agent.md`（主 issue 的正文即该规格）。
- 架构决策：`docs/adr/`。
- 领域术语：`docs/glossary.md`。代码、提交说明与文档使用术语表词汇。
- 客户端命令、Storybook 画框与截图：`client/README.md`。

## 目录

| 路径 | 职责 |
| --- | --- |
| `crates/agent` | agent 二进制；协议类型与 `ts-rs` 导出在此 |
| `crates/ui` | ui 二进制；编译期内嵌 `client/dist` |
| `client` | React + Vite + TypeScript 客户端 |
| `docs` | 规格、ADR、术语表 |

## 命令

所有命令经 `devenv shell --` 执行。首次使用先安装客户端依赖：`devenv shell -- bash -c 'cd client && npm ci'`。

| 命令 | 作用 |
| --- | --- |
| `devenv shell -- durable-build` | 先构建 client，再构建 Rust workspace |
| `devenv shell -- durable-test` | 提交前的全部检查 |
| `devenv shell -- durable-typegen` | 重新生成 TS 协议类型 |
| `devenv shell -- storybook` | Storybook 开发服务器 |

`durable-test` 依次执行 `cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`、TS 类型一致性、ESLint、`tsc --noEmit`、Vitest。提交前必须全部通过。

## 构建顺序

`ui` 在编译期读取 `client/dist`，缺失或过期时行为不可预期。任何 Rust 构建之前先构建 client；`durable-build` 已按此顺序执行。

## 生成物

`client/src/api/generated/` 由 agent 的 `ts-rs` 导出生成，不手写。Rust 协议类型变更后运行 `devenv shell -- durable-typegen`，并把生成文件的变更一并提交。

## 前端约定

- 组件只消费 `client/src/styles/tokens.css` 的语义 token，不写颜色字面量与硬编码长度。
- 页面不含注释性说明。
- 组件样式由确定性检查与截图脚本断言：见 `docs/adr/0010-client-style-verification.md`。

## 工作区噪声

工作区包含 devenv 与构建产物目录：`.devenv/`、`target/`、`client/dist/`、`client/storybook-static/`、`client/screenshots/`。扫描仓库使用 `rg` 或 `git ls-files`，不要用 `find .`。
