# ADR 0005: 无审批、限定 workspace 根的工具执行

- 状态：已接受
- 日期：2026-02-14

## 背景

最小实现不做权限确认，但工具可写文件并执行命令。

## 决策

`read_file`、`write_file`、`edit_file`、`bash` 全部自动执行，不提供审批状态。路径约束由 workspace root containment 保证。bash 以 `sh -c` 执行，cwd 限定根内，超时或取消时杀掉进程组。不提供容器沙箱与网络限制。

## 后果

工具结果中的错误回传模型，run 继续。安全边界是本机单用户与 workspace 根，不防御恶意模型或命令。
