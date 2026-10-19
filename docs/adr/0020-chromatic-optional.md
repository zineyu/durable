# ADR 0020: Chromatic 门禁按 token 存在启用

- 状态：已接受
- 日期：2026-11-02

## 背景

ADR 0008 把 Chromatic 发布列为 CI 门禁，但仓库没有 `CHROMATIC_PROJECT_TOKEN`，Chromatic 步骤以「Missing project token」失败并让整个 CI 变红。视觉门禁的其余部分已经由本地截图脚本覆盖（ADR 0010）。

## 决策

CI 在 job 级注入 `CHROMATIC_PROJECT_TOKEN`，Chromatic 步骤仅在 `env.CHROMATIC_PROJECT_TOKEN != ''` 时执行。没有 token 时不发布、不阻塞，其余步骤照常执行。配置 secret 后同一步骤自动启用，无需改动工作流。

## 后果

- 未配置 token 时 CI 为绿，但视觉变更没有服务端门禁；本地截图脚本与 story 列表仍是当前唯一的视觉检查。
- token 由仓库维护者提供，不由实现方创建。
