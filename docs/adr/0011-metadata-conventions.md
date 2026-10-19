# ADR 0011: workspace 与 session 元数据的持久化约定

- 状态：已接受
- 日期：2026-10-19

## 背景

规格要求 workspace 身份为规范化绝对路径，session 元数据落 SQLite 并在重启后恢复，删除 session 产生事实事件、同时删除该 session 的事件。规格没有规定规范化的具体方式、删除时事实事件与新写事件的先后关系，也没有规定协议字段的命名与 64 位整数的 TS 映射。

## 决策

**workspace 身份**：注册时把路径解析为绝对路径并执行 `canonicalize`，解析符号链接后必须是目录；存储值为该规范化路径，也是 bootstrap 返回的路径。重复注册按规范化路径判断。移除 workspace 按存储值精确匹配，不访问磁盘，因此不要求目录仍然存在；未注册的路径返回 `workspace_not_found`。

**删除 session 保留删除事实**：删除在单个事务内按顺序执行——删除该 session 的全部事件、删除 session 行、写入 `session_deleted` 事实事件。每个已删除 session 因此只保留一条事件。规格的「删除其事件」与「删除产生事实事件」由此同时成立：历史事件清除，终止事实保留，带 `after` 游标的重连客户端可以回放删除。

**协议命名与数值映射**：协议字段使用 camelCase（`#[serde(rename_all = "camelCase")]`），事实事件的 `kind` 使用 snake_case。时间戳与事件 id 是 i64，取值小于 JS 安全整数上限，TS 侧用 `#[ts(type = "number")]` 映射为 `number`。

## 后果

- 重启后 bootstrap 与消息树端点返回的元数据与重启前逐字节一致（含 `updatedAt`）。
- workspace 未注册（`workspace_not_found`）、路径无效（`workspace_path_invalid`）是两类不同错误。
- 每个已删除 session 在事件表留下一条记录；事件量有界。
- 事务内的三步顺序是契约的一部分，不能调整为「先写终止事实，再清历史事件」。
