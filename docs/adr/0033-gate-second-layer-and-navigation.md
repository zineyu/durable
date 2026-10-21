# ADR 0033: 门禁第二层：规模、依赖、PR 自锁与仓库导航

- 状态：已接受
- 日期：2026-10-21

## 背景

ADR-0032 让 lint 告警阻断并禁用抑制，覆盖的是「写错的代码」。它不覆盖另外三类问题，因为这些不是单个文件的语法或类型问题：

- 门禁可以被放松。放松门禁的改动与功能改动可以落在同一个 change 里，审查者看不出差别。
- 文件可以无限变大。文件越长，agent 定位与局部改动的成本越高。
- 文档、规格、类型与术语会与代码漂移，漂移没有信号。

## 决策

### 门禁进版本库，分快慢两条 lane

`devenv.nix` 只做转调，真实门禁是版本库里的脚本：`scripts/lint-fast.sh` 与 `scripts/lint.sh`。慢 lane 先跑快 lane，再跑 `clippy`、`rustdoc`、`cargo-machete`、`cargo-deny`、`typos`、`taplo`、`actionlint`、`shellcheck`、`nixfmt`，最后按文件名跑 `scripts/check-*.sh`。新增检查只需放进 `scripts/`。

快 lane 只含秒级检查：`cargo fmt --check`、`cargo check --workspace --all-targets`、ESLint、stylelint、`tsc --noEmit`。它供连续编辑时反复运行。

### 源码规模预算

单个文件的上限：`crates/*/src` 下 Rust 600 行，`client/src` 下 TS/TSX 400 行，排除 `*.test.ts(x)` 与 `src/api/generated/`。存量超限登记在 `scripts/source-budget.allow`，登记值只许变小；文件回到默认上限以内时必须删除登记行。

这是欠账表，不是豁免：登记值变大即失败。

### 依赖白名单

`deps.allow` 逐行登记每个包的直接依赖与理由，含 dev 依赖。`scripts/check-deps.sh` 用 `cargo metadata --no-deps --locked` 比对，未登记与已失效两个方向都失败。理由写在登记行上，因为「为什么依赖这个 crate」只在新增的那一刻清楚。

### 门禁自锁放在 PR 上，不放在 lint 里

`.github/workflows/gate-watch.yml` 在 `pull_request` 上比对基线与 HEAD 的门禁文件：`scripts/lint*.sh`、`scripts/check-*.sh`、`clippy.toml`、`deny.toml`、`rustfmt.toml`、`client/eslint.config.js`、`client/tsconfig.json`、`client/stylelint.config.mjs`、`Cargo.toml` 的 `[workspace.lints]` 段，以及 `.github/` 全部。命中时在 PR 上发一条评论列出改动文件。评论带固定标记：重复运行更新同一条，改动撤销时删除。

`Cargo.toml` 只比较 `[workspace.lints]` 段，因为依赖版本升级每月都有，整文件比较会把它们全部算成门禁改动。

不做「提交信息必须含标记」的阻断式自锁。原因有两条：本地 `durable-test` 每次提交都跑，阻断会把正常改动卡住；提交信息是自由文本，用字符串匹配约束它是脆弱门禁。审查者看到提醒即可。

## 契约一致性

- 规格追溯：代码与测试写 `规格 user story N`，`scripts/check-spec-links.sh` 断言该编号在 `docs/specs/*.md` 中存在。未被引用的编号只打印清单，不阻断，因为规格先行是常态。
- 类型清单：`docs/api/types.md` 与 TS 类型由同一命令 `scripts/typegen.sh` 生成，`durable-test` 重新生成后比对两个路径。清单从生成文件派生，不重复类型定义。
- 类型导出覆盖：`scripts/check-typegen-coverage.sh` 收集 `crates/*/src` 里每个 `#[derive(TS)]` 的类型名，断言 `client/src/api/generated/<Name>.ts` 存在。生成文件一致只能证明「导出的都正确」，覆盖检查补上「该导出的都导出了」这一半。
- 术语表：`scripts/check-glossary.sh` 断言 `docs/glossary.md` 里标识符形状的反引号术语能在 `crates/` 或 `client/` 里找到。URL 模板一类自然语言片段不参与检查。

## 可导航性

- `docs/adr/README.md` 是 ADR 索引：编号、标题、状态、结论一句。结论压缩自各篇「决策」段。
- `AGENTS.md` 增加 crate 模块导航与「任务 → 文件」映射。agent 从任务直接跳到落点，不必先遍历目录。

## 备选

- **把自锁写成 `scripts/check-*.sh`**：本地每次 `durable-test` 都要与远端基线比较，而工作副本与 `origin/main` 常常相差多个 change，结果是每次提交都要求同一个标记，徒增噪声。
- **用 `deny.toml` 的 `[bans]` 表达依赖白名单**：它约束传递依赖与版本，不约束「某个包可以直接依赖谁」。
- **启用 `clippy::missing_docs` 代替导航文档**：强制写文档注释只会产出复述函数名的注释，不解决「该改哪个文件」。

## 后果

- 门禁变更在 PR 上可见，审查者不必猜。
- 已登记的大文件不能继续变大；要加内容就得先拆分。
- 新增直接依赖需要一次显式登记并写理由。
- 文档漂移会失败或提醒：类型清单与术语不一致即失败，规格编号越界即失败。
- 门禁脚本自身受 `shellcheck` 约束。
- 已知缺口：规模预算只拦增长，不强制已登记文件变小；规格编号是全局集合，两个规格的编号空间重名，因此只能判断「编号存在」，不能判断「引用的是正确的规格」。
