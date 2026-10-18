# ADR 0006: 单一 OpenAI 兼容 provider 与显式上下文失败

- 状态：已接受
- 日期：2026-02-14

## 背景

最小实现需要可本地验证的模型接入。

## 决策

内部定义窄 trait：发起一轮带工具的流式请求。v1 只实现 OpenAI 兼容 `/v1/chat/completions`。同一 assistant 消息的多个 tool_call 顺序执行。最大步数默认 50。不做上下文压缩：超过字符上限时 run 以明确错误结束。system prompt 内置，不支持自定义，不支持 per-session 模型。

## 后果

Anthropic 原生协议后续以第二个实现接入，trait 不暴露供应商细节。长会话需要用户自行新建 session。
