# ADR 0057: 外部通道门禁

- 状态：已接受
- 日期：2026-11-04

## 背景

ADR 0032 让 lint 告警阻断，覆盖「写错的代码」。ADR 0033 让门禁本身可见，覆盖「放松门禁的改动」。两者都不回答另一个问题：谁在碰外部世界。

一个 coding agent 最省力的做法是就地加一条通道：在组件里写一次 `fetch`，在某个模块里 `tokio::spawn` 一条后台任务，在工具之外直接 `std::fs::write`。这些改动编译通过、测试通过、评审时混在功能改动里，唯一的信号是审查者恰好看见。

本仓库已经把多数通道收在单一入口：模型请求只在 `agent/src/provider.rs`，进程只在 `agent/src/link`，文件写入只在 `executor` 的四个工具与 `core` 的存储，客户端请求只在 `apps/client/src/api`。缺的是让「第二条通道」在提交前失败。

## 决策

### 四类通道，一张白名单

`scripts/check-external-channels.sh` 在 `crates/*/src` 与 `apps/*/src` 里识别四类通道，命中必须登记在 `scripts/external-channels.allow`：

| 类别 | 识别什么 | 例子 |
| --- | --- | --- |
| `network` | 建立连接或监听 | `reqwest`、`TcpListener`、`axum::serve`、`fetch(`、`EventSource` |
| `subprocess` | 起子进程 | `Command::new`、`process::Stdio`、`tokio::process`、`libc::kill` |
| `file-write` | 写文件或持久化 | `fs::write`、`create_dir_all`、`File::create`、`RollingFileAppender`、`localStorage` |
| `background` | 让它跑在当前调用之外 | `tokio::spawn`、`spawn_blocking`、`thread::spawn`、`JoinSet` |

识别式只认建立通道的调用，不认类型名与模块名：`std::net::SocketAddr`、`process::ExitCode` 不命中，`std::net::TcpStream` 命中。识别之前切掉注释，整行注释直接丢弃；行内注释切到第一处 `//` 之前，`//` 前面是 `:` 时保留，因此文档里的 `https://` 不算注释起点。

### 登记粒度是「文件 + 类别」

白名单一行是 `「<路径> <类别> # 理由」`。只按文件登记不够：已经因为 `reqwest` 登记的文件，再补一条 `tokio::spawn` 不会被发现。加上类别之后，同文件新增另一类通道仍然失败。

三个方向都失败：未登记的命中报出文件、行号、类别与原文；登记了但没有命中的条目要求删除；格式不对、类别未知、重复登记同样失败。最后一条防止白名单变成只增不减的豁免表。

### 范围是产品代码

检查 `crates/*/src` 与 `apps/*/src`，排除 `*.test.ts(x)`、`*.stories.tsx`、`*.fixture.ts` 与 `apps/client/src/api/generated/`。`crates/*/tests` 不在范围内。

理由是信号与噪声之比：测试不进产品，而每个集成测试都要启动 agent 二进制、建临时目录、绑随机端口。把它们全部登记进来，白名单会翻倍，真正的第二条通道反而更难看见。`crates/*/src` 与 `apps/*/src` 里的 `#[cfg(test)]` 模块仍在范围内，因为排除它需要对每个文件做语法判断，而这类命中很少（当前只有 `executor/src/tools/paths.rs` 一处）。

### 与单一写入线同层

脚本命名为 `scripts/check-*.sh`，因此 `scripts/lint.sh` 收尾的循环自动运行它，与规模预算、依赖白名单、术语表检查同层，包含在 `durable-test` 内，CI 与本地一致。

## 备选

- **用 clippy 的 `disallowed_methods` / `disallowed_types`**：只覆盖 Rust，客户端完全漏掉；而且逐处豁免要写 `#[expect]`，生产代码不允许（ADR 0032）。
- **只按文件登记**：见「登记粒度」。
- **把测试也纳入**：见「范围」。
- **用 `cargo geiger` 一类依赖级工具**：它统计 crate 的不安全代码与依赖，不区分「谁建立了通道」，也不管 TypeScript 与 shell。
- **运行期沙箱（Landlock、`sandbox-exec`）**：能真正限制通道，但两端平台语义不同，macOS 侧已废弃且不可测；它是运行期防线，门禁是提交期防线，两者不互相替代。

## 后果

- 新增一条外部通道要付一次显式登记，登记行必须写清理由，且与代码在同一次提交里。
- 本次基线：22 个文件、24 条登记。此后新增的每一条都要单独过审。
- 识别式是正则，只认识直接调用。经 trait 参数传入的客户端、宏生成的调用、以及字符串里拼出来的通道都不命中；它拦的是「顺手加一条」，不是有意绕过。
- 白名单里的理由会随文件职责漂移而失效；失效的检测只有「条目还有没有命中」，理由是否仍然准确仍靠审查。
- 新的检查脚本自动进入慢 lane，`durable-lint` 因此多一次 `ripgrep` 扫描，耗时在秒级以下。
