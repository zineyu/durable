import type { ContentPart } from '../api/generated/ContentPart';

/// 拼接消息里的文本块。推理块与工具块不属于可见正文。
export function bodyText(parts: ContentPart[]): string {
  let text = '';
  for (const part of parts) {
    if (part.kind === 'text') {
      text += part.text;
    }
  }
  return text;
}
