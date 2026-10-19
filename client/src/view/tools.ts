import type { ToolCallRecord } from '../api/generated/ToolCallRecord';
import type { ToolState } from './types';

export type ToolCardModel = {
  id: string;
  name: string;
  state: ToolState;
  summary: string;
  input: string;
  result: string | null;
};

export function toolCardModel(record: ToolCallRecord): ToolCardModel {
  const outcome = record.outcome;
  return {
    id: record.id,
    name: record.name,
    state: outcome.status === 'succeeded' ? 'succeeded' : 'failed',
    summary: summarize(record.arguments),
    input: record.arguments,
    result:
      outcome.status === 'succeeded' ? outcome.content : `${outcome.code}: ${outcome.message}`,
  };
}

/// 摘要取工具入参中的目标路径或命令；入参不是 JSON 对象时原样返回。
function summarize(argumentsText: string): string {
  const parsed = parseObject(argumentsText);
  if (parsed === null) {
    return argumentsText;
  }
  const path = parsed['path'];
  if (typeof path === 'string') {
    return path;
  }
  const command = parsed['command'];
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
