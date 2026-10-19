# ADR 0012: durable 内核与持久化边界

- 状态：已接受
- 日期：2026-10-20

## 背景

参照 `@earendil-works/pi-durable`，durable 的语义要求：所有可见进度先落库再发布；一次提交跨所有记录原子生效；提交序列单调递增；进程中断后能从已提交状态继续。本仓库原先把 SQLite 访问直接写在 `agent` 的 HTTP 处理器旁，没有提交批次、消息条目与全局 id 分配，无法承载 run、队列与恢复。

## 决策

**新增 `crates/core`（包名 `durable`）承载领域内核**。内核拥有 workspace、session、消息条目与事实事件的持久化；`agent` 只保留协议、进程配置与 HTTP/SSE 外壳。协议类型定义在 core 并派生 `ts-rs`，导出入口仍在 `agent`，生成文件路径不变。

**单一写入线**：`Store` 用异步互斥量串行化所有读-改-写。写入由私有字段的 `Changeset` 登记，只经方法构造，顺序即意图；`Storage::apply` 在一条 SQL 事务内按序执行全部变更。事务提交成功前不广播事实事件，提交后由调用者返回结果。

**消息身份由存储的全局分配器产生**：`meta.next_message_id` 行在自增后返回旧值，因此消息 id 全局单调递增、跨 session 共享、重启后继续；分配成功但事务失败的 id 形成空洞，永不复用。session 身份继续使用 UUID v7 字符串。

**存储用 `sqlx` 的 SQLite 驱动**（`runtime-tokio` + `migrate`），连接强制 `journal_mode=WAL`、`synchronous=FULL`、`foreign_keys=ON`，连接池上限 1。迁移文件随二进制内嵌，启动时自动应用。`libsqlite3-sys` 的 `bundled` 特性保证不依赖系统 SQLite。

**编辑器版本与生态**：workspace 使用 edition 2024；错误用 `thiserror`，时间戳用 `jiff`，异步运行时与互斥量用 `tokio`。测试改用 `sqlx` 打开同一个数据库，删除 `rusqlite` 依赖，避免两个 `links = "sqlite3"` 的包冲突。

## 后果

- 消息条目、active leaf、session 更新时间与事实事件在一次事务内同时可见；重启后逐字节一致。
- 内核不依赖 HTTP 与 axum，seam 1 仍经 `agent` 的 HTTP API 驱动。
- 后续 run、队列与 task 检查点经同一 `Changeset` 扩展，新增变更种类即可，不改存储事务边界。
- 读取路径与写入线分离：读取不加锁，写入串行；`synchronous=FULL` 以写吞吐换取崩溃后可恢复的提交。
