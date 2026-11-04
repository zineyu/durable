# durable

本机单用户的 coding agent。`agent` 提供 HTTP API 与内嵌的 React 客户端，工具在 host 上由 `executor` 执行。

## 布局

仓库是 monorepo，分三类目录：

| 目录 | 内容 |
| --- | --- |
| `apps/` | 可部署端：`agent`、`executor`、`client` |
| `crates/` | 库 crate：`core`（包名 `durable`）、`runtime`、`effects` |
| `packages/` | 将来被多个端共享的 Node 包。现在没有包 |

`scripts/check-layout.sh` 钉住这条边界：`apps/` 与 `packages/` 下的每个目录必须是被发现的包，`crates/` 下不得出现可执行端。理由见 `docs/adr/0058-monorepo-layout.md`。

## 开始

工具链与依赖版本由 `devenv.nix` 锁定。所有命令经 `devenv shell --` 执行。

```
devenv shell -- bash -c 'cd apps/client && npm ci'
devenv shell -- durable-build
devenv shell -- durable-test
```

`durable-build` 先构建 Node 包，再构建 Rust 包。`agent` 在编译期内嵌 `apps/client/dist`，顺序反过来就编译失败。

## 命令

| 命令 | 作用 |
| --- | --- |
| `devenv shell -- durable-build` | 构建全部包 |
| `devenv shell -- durable-lint` | 全部静态检查，不含测试 |
| `devenv shell -- durable-lint-fast` | 只含秒级静态检查 |
| `devenv shell -- durable-lint-package <包名>` | 只检查一个包 |
| `devenv shell -- durable-test` | 提交前的全部检查 |
| `devenv shell -- durable-e2e` | 浏览器端到端测试 |
| `devenv shell -- durable-typegen` | 重新生成 TS 协议类型 |
| `devenv shell -- storybook` | Storybook 开发服务器 |

包由发现得到：`scripts/task.sh --list` 打印包、目录与它能跑的任务。新增一个包只需要放进 `apps/` 或 `packages/`，并带上 `Cargo.toml` 或 `package.json`。

## 文档

| 文件 | 内容 |
| --- | --- |
| `AGENTS.md` | 目录、模块导航、门禁、约定。改动仓库之前先读 |
| `docs/specs/` | 规格 |
| `docs/adr/` | 架构决策，索引见 `docs/adr/README.md` |
| `docs/glossary.md` | 领域术语 |
| `apps/client/README.md` | 客户端命令、Storybook 画框与截图 |
