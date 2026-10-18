# ADR 0008: 生成式协议类型与 CI 门禁

- 状态：已接受
- 日期：2026-02-14

## 背景

Rust 与 TS 之间的契约必须可验证一致。

## 决策

ts-rs 从 Rust 类型生成 TS，生成文件提交到 `client/src/api/generated/`，CI 重新生成后 diff 必须为空。CI 依次执行 cargo fmt、clippy、cargo test、类型一致性检查、npm lint、typecheck、vitest、storybook build、Chromatic 发布与视觉门禁。

## 后果

Chromatic 需要仓库 secret `CHROMATIC_PROJECT_TOKEN`，当前仓库尚未配置 remote，属于前置依赖。
