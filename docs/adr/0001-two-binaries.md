# ADR 0001: C/S 分离为 agent 与 ui 两个二进制

- 状态：已接受
- 日期：2026-02-14

## 背景

需要把 coding agent 与 web UI 分离，同时避免浏览器跨域与鉴权复杂度。

## 决策

仓库产出两个 Rust 二进制：`agent` 只提供 HTTP API；`ui` 内嵌构建后的 React 静态资源并托管，同时把 `/api/*` 反代到 agent。两者独立启动、独立配置地址。

## 后果

浏览器始终同源访问 ui，无需 CORS。ui 的代理必须对 SSE 流式透传，不得缓冲。ui 构建依赖 client 构建产物。
