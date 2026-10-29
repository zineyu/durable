# ADR 0032: 严格 lint 门禁

- 状态：已接受
- 日期：2026-10-21

## 背景

本仓库的主要作者是 coding agent。agent 的典型错误不是编译失败，而是二类：绕过检查的写法（`unwrap`、静默兜底、把校验压成注释）与悄悄失效的契约（空分支、未遍历的联合类型）。默认 lint 抓不到这两类。

改动前的门禁只有 `cargo clippy`（`clippy::all`）与 ESLint `recommended`（非类型感知）。告警不阻断；agent 可以用一条 `#[allow]` 或 `eslint-disable` 消掉任何提示。

## 决策

### 一条命令，十类检查

`durable-lint` 是唯一入口。它依次运行：

| 检查 | 工具 | 覆盖 |
| --- | --- | --- |
| Rust 格式 | `rustfmt` | `rustfmt.toml` |
| Rust 语义 | `clippy` `all` + `pedantic` + restriction | `[workspace.lints]` 与 `clippy.toml` |
| Rust 文档 | `rustdoc`（`--lib`） | 文档链接、失效引用与 rustdoc lint |
| Rust 依赖缺口 | `cargo-machete` | 未被使用的依赖 |
| Rust 依赖合规 | `cargo-deny` | 漏洞、yanked、通配版本、许可证、来源 |
| TS/TSX | `ESLint` `strictTypeChecked` + `stylisticTypeChecked` + `react-hooks` | 类型感知 |
| TS 类型 | `tsc --noEmit` | `tsconfig.json` 全部严格开关 |
| CSS | `stylelint` `stylelint-config-standard` | 语法、废弃关键字、颜色字面量 |
| 拼写 | `typos` | 全部文本 |
| 仓库配置 | `taplo`、`actionlint`（用 `shellcheck` 检查 `run:` 脚本）、`nixfmt` | TOML、GitHub Actions、Nix |

`durable-test` 先跑 `durable-lint` 再跑测试。CI 跑 `durable-test`，因此门禁没有旁路。

### 禁止压掉检查

抑制只能经以下四条机制出现，其余一律禁止：

- Rust 一律 `#[expect]`，不用 `#[allow]`。由 `clippy::allow_attributes` 强制。
- 每个 `#[expect]` 必须带 `reason`。由 `clippy::allow_attributes_without_reason` 强制。
- `#[expect]` 在 lint 不再触发时自身报错。过时的抑制无法留在代码里。
- ESLint 的 `reportUnusedDisableDirectives` 设为 `error`，`--max-warnings=0`，`clippy` 用 `-D warnings`。失效的 `eslint-disable` 是错误，任何告警都阻断。

### 生产代码的四处置信

`crates/*/src/` 只有四处 `#[expect]`，全部与运行环境边界有关。逐条列出，便于审查（`crates/hub` 删除后原有三处随 crate 消失，见 ADR 0048）：

| 位置 | lint | 理由 |
| --- | --- | --- |
| `runtime/src/logging.rs` `parse_filter` | `print_stderr` | 过滤器在订阅者建立之前校验，此时日志不可用。 |
| `agent/src/main.rs` `report_startup_failure` | `print_stderr` | 启动失败时日志尚未初始化。 |
| `agent/src/main.rs` `report_listen_address` | `print_stdout` | 监听地址是启动协议，启动测试与浏览器端到端测试从标准输出读取。 |
| `executor/src/tools/bash.rs` `ProcessGroup::kill` | `unsafe_code` | 标准库没有杀进程组的接口，只能调 `libc::kill`；块上有 `// SAFETY:` 说明前提。 |

测试脚手架是另一类。集成测试的共享模块不是 `#[test]` 函数，`clippy` 不把它算作测试代码。这些模块在文件头用一条 `#![expect(...)]` 覆盖 `expect_used`、`panic`、`indexing_slicing` 与 `let_underscore_must_use`，并写明理由：夹具失败必须立刻可见。禁止逐行散放抑制。

### 配置层的三条例外

记录在此，避免被当成遗漏：

- `@typescript-eslint/no-unnecessary-condition` 关闭。TypeScript 把闭包内改写的存活标志错定为常量，`await` 之后重复的存活检查被判为死代码；这些检查在断线重连路径上必需。理由写在 `client/eslint.config.js`。
- `clippy::cargo_common_metadata` 与 `clippy::multiple_crate_versions` 允许。前者要求 crates.io 元数据，本仓库不发布；后者是上游依赖树里 `syn` 2 与 3 并存，无法在不降级依赖的前提下消除。
- 未启用 `clippy::nursery`。它的语义会随工具链变动，不适合当作阻断门禁。

### 范围

检查代码与仓库配置，不检查文档与 SQL。36 个 Markdown 文档是规格与决策记录，结构由人手写维护；5 个迁移文件的正确性由集成测试覆盖。

## 备选

- **只加 `-D warnings`**：现有告警立刻阻断，但 `pedantic` 与类型感知规则仍关着，抓不到空分支与未校验的 `any`。
- **开 `clippy::nursery` 与 `clippy::restriction` 全组**：`restriction` 组内规则互相矛盾，全开会产出数百条互相抵触的告警，最后一定靠大范围抑制收场。
- **引入非官方 React 规则集**：`@eslint-react` 提供 140 条规则，实测只多出 2 条与官方 `react-hooks` 重复的告警和 2 条风格意见，收益不抵维护面。
- **用 `eslint-plugin-jsx-a11y`**：该插件的 peer 上限是 ESLint 9，本仓库用 ESLint 10。强行安装须加 `--legacy-peer-deps`，等于让依赖树说谎。

## 后果

- agent 写出的 `unwrap`、`expect`、`panic`、索引切片、未处理联合分支、类型逃逸在提交前失败。
- 抑制必须带理由且可失效，因此「把告警压掉」不再是可行路径。
- 门禁变慢：`durable-lint` 含一次 `cargo clippy --all-targets` 与一次 `cargo deny`（首次需要 advisory 数据库）。
- 无障碍（a11y）目前没有检查器。等 `eslint-plugin-jsx-a11y` 支持 ESLint 10 后补上，或改用支持当前 ESLint 的替代品。
- `fmt`、重构一类正交修改会因为格式规则收紧而产生一次性大 diff；本次已一次性拉平。
