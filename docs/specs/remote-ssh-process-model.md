# 规格：remote-ssh 形状的进程模型

> 主 issue: #56

## Problem Statement

我在几台自有机器上工作，今天每台机器都要跑一个 `agent`，于是：

- 模型密钥与 provider 配置要在每台机器上各配一份。谁执行工具，谁就必须知道模型凭据。
- 会话历史按机器分开。我在笔记本上读不到服务器上的历史，换一台机器就要另开一个页面。
- 我想让笔记本持有模型调用与全部历史，让服务器只负责读写文件，今天没有这条路，因为 agent 循环与工具执行在同一个进程里。
- `hub` 是总线。它引入的节点表、合并协议、广播序号、回放窗口与客户端分片，存在的唯一理由是把各机器分散的状态拼到一个页面里。

## Solution

照 VS Code Remote SSH 的形状改造：

- `agent`（原 `hub`）独占 agent 循环、provider 调用、SQLite、事件流与浏览器入口。它是唯一权威。
- 每台提供 workspace 的机器是一个 `host`。host 上只跑 `executor`：一个只执行工具的小进程，没有数据库，没有模型配置。
- `agent` 通过 SSH 把 `executor` 安装到 host，并在一条 link 上与之通信。link 是标准输入输出上的 JSON 帧。
- workspace 绑定 host，session 属于 workspace。工具调用的路径校验与执行全部发生在 host 上。
- 单机使用不需要 SSH：本机 host 的 link 是一条直接启动的 `executor` 子进程。
- 客户端回到一条事件流、一个游标与一份状态。URL 变成 `#/s/<session_id>`。

## User Stories

### host 管理

- **R-1** 作为用户，我想注册一个 host，只填名称与 SSH 目标，以便接入一台机器。
- **R-2** 作为用户，我想让 SSH 目标写成 `~/.ssh/config` 里的别名，以便端口、密钥与跳板继续由 SSH 配置提供。
- **R-3** 作为用户，我想在名称重复时收到显式错误，以便 host 身份不含糊。
- **R-4** 作为用户，我想在名称为空或 SSH 目标为空时收到显式错误，以便 host 表始终可用。
- **R-5** 作为用户，我想改 host 名，以便名字贴合当前用途。
- **R-6** 作为用户，我理解改名会让旧名立即变成未知 host，以便我不误以为旧链接还能用。
- **R-7** 作为用户，我想改 host 的 SSH 目标，以便机器换端口或换地址时不用删除再注册。
- **R-8** 作为用户，我想停用一个 host，以便关机的机器不再被重连。
- **R-9** 作为用户，我想删除一个 host 只移除登记，以便那台机器上的 workspace 文件与进程不受影响。
- **R-10** 作为用户，我想手动触发一次重连，以便我确定隧道已经通了。
- **R-11** 作为用户，我想看到每个 host 的状态 `ok`、`unreachable`、`incompatible`，以便一眼分出「能用」「连不上」「版本不对」。
- **R-12** 作为用户，我想看到状态说明，以便我知道是网络断了、SSH 认证失败还是安装失败。
- **R-13** 作为用户，我想看到每条 link 的往返延迟，以便判断链路质量。
- **R-14** 作为用户，我想在首次连接时自动安装 `executor`，以便 host 上不需要任何预装步骤。
- **R-15** 作为用户，我想在安装失败时看到失败原文，以便我知道是磁盘满了还是权限不对。
- **R-16** 作为用户，我想在 `executor` 的构建版本或帧协议版本与 `agent` 不符时看到 `incompatible`，以便我升级落后的一侧。
- **R-17** 作为用户，我想在本机第一次启动时自动得到一个名为 `local` 的 host，以便单机使用不需要任何注册步骤。
- **R-18** 作为用户，我想让本机 host 用子进程而不是 SSH，以便我不必打开系统的远程登录。
- **R-19** 作为用户，我想在 host 停用时看到它的 workspace 与 session 不可操作，以便我不发出注定失败的写操作。

### workspace 与 session

- **R-20** 作为用户，我想把一个绝对目录注册到指定 host，以便 session 在该机器的目录里工作。
- **R-21** 作为用户，我想让路径校验在 host 上完成，以便「路径不存在」「不是目录」的判定基于真实文件系统。
- **R-22** 作为用户，我想在 host 不可用时收到 `host_unavailable`，以便我分清「机器离线」与「路径写错」。
- **R-23** 作为用户，我想在一个 host 上注册多个 workspace，以便一台机器上同时工作几个项目。
- **R-24** 作为用户，我理解一个 workspace 只属于一个 host，以便「工具在哪台机器上跑」不需要再问。
- **R-25** 作为用户，我想移除 workspace 只删除登记，以便磁盘上的文件不受影响。
- **R-26** 作为用户，我想让 session 继承其 workspace 的 host，以便同一个会话不会在机器之间漂移。
- **R-27** 作为用户，我想在侧栏按 host 分组看到 workspace 与 session，以便分辨哪台机器。

### 工具执行

- **R-28** 作为用户，我想让 `read_file`、`write_file`、`edit_file`、`bash` 都在 host 上执行，以便文件与命令作用于那台机器。
- **R-29** 作为用户，我想让工具的文件副作用出现在 host 的目录里，以便结果可以直接查看。
- **R-30** 作为用户，我想让路径逃逸（绝对路径、`..`、符号链接）在 host 上被拒绝，以便安全边界只有一处判定。
- **R-31** 作为用户，我想让 `bash` 的 cwd、超时、进程组与输出截断语义与今天一致，以便切换机器不改变工具行为。
- **R-32** 作为用户，我想让工具失败照旧回传模型并让 run 继续，以便远端失败不改变对话流程。
- **R-33** 作为用户，我想在我取消 run 时让 host 上的进程组被杀掉，以便取消真的停下命令。
- **R-34** 作为用户，我理解工具输出在调用结束时一次出现，以便界面不出现半个工具卡片。
- **R-35** 作为用户，我想让同一台 host 上的多个 session 共用一个 SSH 连接，以便开多个会话不必开多条链路。

### 失败与恢复

- **R-36** 作为用户，我想在 link 断开时让正在等结果的 run 以 `host_unreachable` 结束，以便失败显式可见。
- **R-37** 作为用户，我想让断链时那次工具调用的结局是「结果未知」，以便我不把它当成成功或工具失败。
- **R-38** 作为用户，我想让断链后的队列继续执行下一项，以便一次抖动不冻结整个 session。
- **R-39** 作为用户，我想让链路恢复后的新消息照常可用，以便我不必重启。
- **R-40** 作为用户，我想让 `agent` 重启后仍按树尾续跑未完成的 run，以便重启不丢进度。
- **R-41** 作为用户，我想让重启后续跑在 host 离线时以 `host_unreachable` 失败，以便我不等一个不会来的结果。
- **R-42** 作为用户，我想让 `executor` 在标准输入收到 EOF 时杀掉自己启动的进程组并退出，以便链路消失后主机上不留孤儿命令。

### 客户端

- **R-43** 作为用户，我想只面对一条事件流与一个游标，以便断线续传只有一个语义。
- **R-44** 作为用户，我想让 URL 形如 `#/s/<session_id>`，以便分享与收藏不再需要机器前缀。
- **R-45** 作为用户，我想在刷新后恢复选中 session 与滚动位置，以便不丢上下文。
- **R-46** 作为用户，我想在一个面板里注册、改名、删除、停用 host 并手动重连，以便运维不离开页面。
- **R-47** 作为用户，我想让不可达 host 的会话列表为空并显示说明，以便我不把空列表当成「没有会话」。
- **R-48** 作为用户，我想让命令面板按 host 分组，以便快速跳到另一台机器的会话。
- **R-49** 作为用户，我想看到连接状态（已连接、重连中、已断开），以便知道界面是否实时。
- **R-50** 作为用户，我想从多个标签页与多台设备同时打开 `agent`，以便在不同屏幕上协作同一批会话。

### 工程与运维

- **R-51** 作为开发者，我想让 `agent` 一个二进制同时提供 API 与前端资源，以便部署只有一个进程。
- **R-52** 作为开发者，我想让 host 相关的协议类型由 Rust 导出到 TypeScript，以便形状漂移在编译期暴露。
- **R-53** 作为开发者，我想让 `agent` 与 `executor` 的帧协议版本显式握手，以便两侧不同步时是 `incompatible` 而不是随机错误。
- **R-54** 作为运维者，我想让 `executor` 的日志并入 `agent` 的日志并标注 host 名，以便排查只需要看一处。
- **R-55** 作为开发者，我想删除 `hub` 的总线、回放窗口、节点分片与合并 bootstrap，以便代码形状与职责一致。
- **R-56** 作为用户，我想在打开由旧二进制写入的数据库时看到显式错误，以便我不把旧库当成新库。

## Implementation Decisions

### 进程与 crate

- `agent` 吸收原 `hub` 的全部职责与资源：协议路由、SSE、静态资源托管、SQLite、agent 循环、provider 调用、工具调用编排。启动参数为监听地址、可选数据库路径、可选日志目录、可重复的 `--host <name>=<ssh-target>`、可重复的 `--workspace <host>:<path>`。
- 新增 `executor` crate 与二进制。它只做四件事：读取帧、校验路径、执行四个工具、写回结果。它不持有数据库，不读模型配置，不监听端口。日志写标准错误，由 `agent` 并入自己的日志并标注 host 名。
- 删除 `hub` crate，以及内核里的节点领域类型与节点表迁移。
- `core` 承担共享的工具定义与参数类型：`agent` 用它们拼模型请求，`executor` 用它们反序列化与校验。工具的错误码表不变。

### 领域与持久化

- 新增 host 表：`name` 为主键，另有 `kind`（`local` 或 `ssh`）、`target`（`kind` 为 `ssh` 时必填）、`enabled`、`installed_build`、`created_at` 与 `updated_at`。host 名即身份。
- `workspace` 增加 `host` 列。workspace 身份从此是 `(host, 规范化绝对路径)`，唯一约束落在这一对字段上。
- session 不变，它属于 workspace，因而属于该 workspace 的 host。
- 事实事件新增 `host_registered`、`host_removed`、`host_updated`、`host_status_changed`，取代节点的同名事件。事件信封的 `session` 字段与水位语义不变。
- 迁移集合收敛为一个基线：新库一次建成所有表，不再含节点表。打开库时若发现旧结构（存在节点表，或 workspace 缺 host 列），以显式错误拒绝启动，错误说明指出这是旧库并要求改用新路径。不提供导入。
- 默认数据库文件名仍为 `agent.sqlite`。默认日志目录与日志格式不变。

### 对外协议

- 删除节点作用域前缀 `/api/n/<node>/...` 与合并 bootstrap。
- 新增 host 管理端点：注册、`PATCH`（名、目标、启用）、删除、手动重连。读取面由 bootstrap 一次给出 host 表与每个 host 的快照（状态、说明、延迟）。
- workspace 注册请求体变为 `{ host, path }`。
- 新增错误码：`host_not_found`、`host_name_taken`、`host_name_invalid`、`host_invalid_target`、`host_unavailable`。新增 run 失败类别 `host_unreachable`。
- 导出类型：`HostName`、`HostKind`、`HostSnapshot`、`HostStatus`。删除 `Hub` 前缀的类型与节点类型。
- 对外协议版本升一版，客户端与 `agent` 同源发布，不做跨版本兼容。

### link 与帧

- 帧格式为长度前缀（4 字节大端）加 UTF-8 JSON。帧只有四种：`request`、`result`、`cancel`、`ping`。
- `request` 带请求 id、工具名与参数 JSON。`result` 带同一 id、结局（成功、工具失败、取消）与内容。多个请求按 id 多路复用，一个 host 只有一条 link。
- `agent` 与 `executor` 的帧协议版本通过启动参数传递：`agent` 启动 `executor` 时带上协议版本与期望的构建 id。`executor` 发现不符即退出并在标准错误写出原因，`agent` 据此把 host 置为 `incompatible`。
- 远端传输：`agent` 启动 `ssh <target> executor --protocol <n> --build <id>`，以 ssh 子进程的标准输入输出为 link。远端不监听端口。
- 本机传输：`agent` 直接启动 `executor` 子进程，参数相同，标准输入输出为 link。
- 安装：`agent` 记录每个 host 的 `installed_build`。首次连接或构建变化时，把内嵌的对应平台 `executor` 二进制经 SSH 的标准输入写入 host 的标准数据目录下的 `executor-<build>`，加可执行位，然后启动。平台与架构按远端输出选择；不支持的组合直接 `incompatible`。
- 心跳：每 15 秒一次 `ping`，往返毫秒数写入 host 快照的延迟字段。
- 断链：link 读侧结束即视为断开。正在等待结果的 run 以 `host_unreachable` 结束，该工具调用的结局记为「结果未知」，队列继续。`executor` 侧在标准输入 EOF 时杀掉自己启动的全部进程组后退出。

### 工具

- 四个工具的定义、参数类型与错误码留在内核，由两侧共用。
- `agent` 只把模型给出的参数原样放进 `request` 帧，不理解参数形状。参数反序列化、路径解析与执行全部在 `executor` 内。
- 工具输出不在 link 上流式传输：`bash` 在 `executor` 内聚合、截断、计时，结果一次回传。

### 客户端

- 状态形状收敛为：host 表、workspace 列表、session 列表、当前 session 的树、事件游标。删除按节点分片、广播序号、回放窗口与合并 bootstrap。
- URL 采用 `#/s/<session_id>`。session id 全局唯一，因此不需要机器前缀。
- 侧栏按 host 分组 workspace 与 session。host 面板提供注册、改名、删除、停用与手动重连。
- 命令面板的 session 组按 host 归类，host 组列出全部 host 与它们的可用状态。
- 不可用 host 的会话列表显示说明，相关命令禁用。

## Testing Decisions

好的测试只断言外部可观察的行为：HTTP 状态与错误码、事件流内容与顺序、SQLite 重启后的恢复、文件系统副作用。测试不替换被测代码，不断言内部结构与私有函数。

测试 seam 保持三条，全部位于公开边界，全部使用本机传输：

- **Seam 1：`agent` 的 HTTP API。** 真实 `agent`、临时 workspace、临时 SQLite、随机本地端口、脚本化 provider；`agent` 经本机传输启动真实 `executor` 子进程。覆盖：host 表命令与错误码，workspace 注册带 host，四个工具的远端执行与文件副作用，路径逃逸，取消时进程组被杀，link 断开后的 `host_unreachable` 与「结果未知」，队列继续，重启后续跑，host 停用时的 `host_unavailable`。
- **Seam 2：client 纯 reducer。** 交叉调用，输入 bootstrap、事实事件与 session 树快照，断言单一状态与派生视图；覆盖 host 分组、URL 解析、不可用 host 的呈现判定。
- **Seam 3：浏览器端到端。** Playwright 启动真实 `agent`（本机 host、脚本化 provider），覆盖注册 workspace、发消息、工具卡片、取消、刷新恢复与 `agent` 重启后的重连。

`hub` 的第三 seam（节点表、转发与总线）随 crate 一起删除。`executor` 不新增 seam：它的每一条路径都能经 `agent` 的 HTTP 触发。断链用杀掉本地 `executor` 子进程模拟，不需要 sshd。

SSH 传输与安装不做自动化测试。发布检查清单里有一条手工验证：把 `ssh localhost` 作为 host，走完注册、安装、一次工具调用与一次断链，并记录状态与说明。

### 验收标准

- 注册 host 后 bootstrap 返回该 host 与状态 `ok`；停用后状态为 `unreachable` 且说明为「节点已停用」；指向它的 workspace 注册返回 `host_unavailable`。
- 重复名注册返回 `host_name_taken`；空名或空 SSH 目标返回 `host_name_invalid` 或 `host_invalid_target`。
- 注册 workspace 时路径不存在返回既有路径错误；路径校验由 `executor` 完成（用 host 上真实目录验证）。
- 一次 run 中 `bash` 在 host 的 workspace 里创建文件，测试在临时目录直接观察该文件。
- 路径逃逸尝试返回 `path_escape`；符号链接逃逸同样被拒。
- 取消运行中的 run：host 上的 `bash` 进程组被终止，部分文本保留，状态为 `cancelled`，队列继续。
- 杀掉 `executor` 子进程：run 以 `host_unreachable` 失败，该工具结局为「结果未知」，队列继续。
- `agent` 重启仍为 running 的 run 续跑完成；若 host 停用，run 以 `host_unreachable` 失败。
- 打开由旧二进制写入的数据库时启动失败，错误说明指出旧结构。
- 事件流只有一条，游标按事件 id 单调；刷新后视图与 bootstrap 一致。
- e2e：注册 workspace、创建 session、提交消息、看到工具卡片、取消、刷新恢复、`agent` 重启后自动重连。

## Out of Scope

- 旧库的数据导入与兼容读取。
- SSH 传输与安装的自动化测试（含在 CI 里启动 sshd）。
- 端口转发与远端监听端口。
- Windows 作为 host。
- 跨 host 的工具调用：一次 run 的工具全部在 session 所属 workspace 的 host 上执行。
- host 侧的模型调用、离线执行、结果缓存与断链重跑。
- 工具输出的流式传输与流式工具卡片。
- 多个 `agent` 实例共享一个 host，或一个 `agent` 连多台 host 的负载均衡。
- 容器或沙箱隔离、命令审批、网络限制。

## Further Notes

- 决策记录：ADR 0048（进程模型反转为 remote-ssh 形状）取代 ADR 0001 的二进制划分与 ADR 0021 至 ADR 0024 的总线与分片决策。ADR 0015 的工具契约、ADR 0037 的「结果未知」、ADR 0047 的续跑全部保留。
- 规格编号新增 `R-` 前缀。`scripts/check-spec-links.sh` 需要把 `R-` 映射到本文件，并把 `crates/executor` 归到 `R-`，其余 crate 允许 `A-` 与 `R-`。
- 主规格 `docs/specs/rust-react-coding-agent.md` 与 `docs/specs/multi-node-hub.md` 在本规格落地后需要修订：前者关于工具执行位置、`hub` 与 URL 的段落要改，后者整体失效并可删除。
- 建议的纵向切片顺序，每片独立通过 `durable-test`：① 内核收敛迁移基线并新增 host 表与 workspace 的 host 列；② `agent` 吸收静态资源与路由，host 表命令上线，客户端 URL 与分组改造；③ 新增 `executor` crate 与本地传输，工具调用改走帧；④ 删除 `hub` crate 与节点相关代码；⑤ 安装与 SSH 传输；⑥ 文档同步与手工验证清单。
- 默认数据库文件名仍是 `agent.sqlite`，而旧 `agent` 二进制用过同一个名字。旧库因此会以显式错误被拒绝；升级路径是删除旧库或改用 `--database` 指向新路径。
