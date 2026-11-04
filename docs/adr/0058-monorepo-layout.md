# ADR 0058: monorepo 布局与跨包任务编排

- 状态：已接受
- 日期：2026-11-04

## 背景

仓库此前已经是事实上的 monorepo：根 `Cargo.toml` 是 workspace，`client/` 是一个独立的 npm 包。但没有两件东西。

第一，目录形状没有表达「哪些是端」。`crates/` 里同时放库 crate（`core`、`runtime`、`effects`）与两个可执行端（`agent`、`executor`），`client/` 却在自己的顶层。读者要打开 `Cargo.toml` 才知道哪一个 crate 会产出二进制。

第二，任务入口是硬编码的。构建顺序写在 `devenv.nix` 的 `durable-build` 里（先 `cd client && npm run build`，再 `cargo build --workspace`）；客户端依赖装在 `devenv.nix` 的字符串里；`scripts/lint.sh`、`scripts/lint-fast.sh`、`scripts/typegen.sh` 各自写死 `cd client` 与包名；单包 clippy 走 `durable-lint-crate <crate>`，只认识 Rust。新增一个包（例如将来从客户端抽出的协议包）必须同时改五处脚本，漏改一处就静默不跑。

## 决策

### 目录分三类

| 目录 | 放什么 |
| --- | --- |
| `apps/` | 可部署端，每个目录产出一个可运行的产物 |
| `crates/` | 只放库 crate |
| `packages/` | 将来被多个端共享的 Node 包。现在没有包，因此没有这个目录 |

`agent` 与 `executor` 从 `crates/` 移到 `apps/`，`client` 从仓库顶层移到 `apps/client`。`core`、`runtime`、`effects` 留在 `crates/`。

`apps/agent` 与 `apps/executor` 与 `crates/*` 处于同一层深，因此 `apps/agent/build.rs` 里指向 `build/build_id.rs` 的 `../../` 不变，指向客户端产物的路径从 `../../client/dist` 变成 `../../apps/client/dist`。

判据由 `scripts/check-layout.sh` 执行：`apps/` 与 `packages/` 下的每个目录必须是被发现的包，`crates/` 下不得出现 `src/main.rs` 或 `[[bin]]`，包名不得重复。

### 包由发现得到，不由清单列出

`scripts/task.sh` 从两个来源发现包：Rust 包来自 `cargo metadata --no-deps`，Node 包来自 `apps/*/package.json` 与 `packages/*/package.json`。仓库里没有包清单文件，因此不存在「清单与目录不一致」这种失败。

包支持哪些任务也由包自己声明：Node 包看 `package.json` 的 `scripts`；Rust 包的 `build`、`lint`、`test` 恒有，`typegen` 只在存在 `export-types-<包名>` 二进制时成立。没有声明某个任务的包被跳过，不算失败。

任务固定为 `build`、`lint`、`test`、`e2e`、`typegen`。执行顺序固定为 Node 包在前、Rust 包在后：`agent` 在编译期内嵌 `apps/client/dist`，顺序反过来就编译失败。

`--package <包名>` 与 `--kind rust|node` 收窄范围，`--list` 打印发现结果。仓库级检查（`cargo fmt`、`cargo deny`、`typos`、文档、生成物比对）不属于任何一个包，仍留在 `scripts/lint.sh`、`scripts/lint-fast.sh` 与 `scripts/typegen.sh`。

### `test` 与 `e2e` 先构建 Node 包

`apps/agent/build.rs` 在 `apps/client/dist` 缺失时让编译失败。此前这一点靠人记住「先跑 `durable-build`」，或者靠 CI 里的单独一步。现在 `scripts/task.sh test` 与 `scripts/task.sh e2e` 先构建所有 Node 包，再跑测试，全新检出可以直接跑通。

### 命令行名字随包概念走

`durable-lint-crate <crate>` 换成 `durable-lint-package <包名>`：同一个入口同时接受 Rust 包与 Node 包。`devenv.nix` 的其余命令名字不变，行为改为转调 `scripts/task.sh`。

## 后果

- 目录位置与一条命令名变化，属于破坏性变更：`crates/agent`、`crates/executor`、`client` 的旧路径与 `durable-lint-crate` 都不再存在。文档、CI 与门禁脚本里的路径已在同一次变更里更新。
- 新增包不改脚本：放进 `apps/` 或 `packages/` 并带上 `Cargo.toml` 或 `package.json` 即可。
- `crates/` 的边界由门禁钉住，可执行端不会悄悄长回库里。
- `scripts/task.sh` 不做增量判定，也不做并行：包的数量还少，确定性比速度重要。需要时先在 `--package` 上收窄。
