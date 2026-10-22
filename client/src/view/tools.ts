import type { ContentPart } from '../api/generated/ContentPart';
import type { ToolResultState } from '../api/generated/ToolResultState';
import type { ToolState } from './types';

/// assistant 消息里的工具调用块。
export type ToolCallPart = Extract<ContentPart, { kind: 'tool_call' }>;

export type ToolCardModel = {
  id: string;
  name: string;
  state: ToolState;
  summary: string;
  input: string;
  result: string | null;
};

/// 把工具调用块与它的结局合成卡片模型。结局未到时按执行中渲染。
export function toolCardModel(
  call: ToolCallPart,
  state: ToolResultState | undefined,
): ToolCardModel {
  const model: ToolCardModel = {
    id: call.callId,
    name: call.name,
    state: 'running',
    summary: summarize(call.arguments),
    input: call.arguments,
    result: null,
  };
  if (state === undefined || state.status === 'running') {
    return model;
  }
  if (state.status === 'unknown') {
    return { ...model, state: 'unknown', result: '工具结果未知：执行中崩溃，结局没有观测到。' };
  }
  const outcome = state.outcome;
  return outcome.status === 'succeeded'
    ? { ...model, state: 'succeeded', result: outcome.content }
    : { ...model, state: 'failed', result: `${outcome.code}: ${outcome.message}` };
}

/// 摘要取工具入参中的目标路径或命令；入参不是 JSON 对象时原样返回。
function summarize(argumentsText: string): string {
  const parsed = parseObject(argumentsText);
  if (parsed === null) {
    return argumentsText;
  }
  const path = parsed.path;
  if (typeof path === 'string') {
    return path;
  }
  const command = parsed.command;
  if (typeof command === 'string') {
    return command;
  }
  return argumentsText;
}

function parseObject(text: string): Record<string, unknown> | null {
  try {
    const value: unknown = JSON.parse(text);
    if (value === null || typeof value !== 'object' || Array.isArray(value)) {
      return null;
    }
    return value as Record<string, unknown>;
  } catch {
    return null;
  }
}
