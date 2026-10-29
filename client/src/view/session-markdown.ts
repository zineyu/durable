import type { HostName } from '../api/generated/HostName';
import { bodyText } from './content';
import { toolStateLabel } from './labels';
import { toolCardModel } from './tools';
import type { ToolCallPart } from './tools';
import type { SessionView } from './types';

/// 把 session 派生视图的活动链渲染为 Markdown。工具调用折叠为引用块。
export function sessionMarkdown(view: SessionView, host: HostName): string {
  const results = view.toolResults;
  const sections: string[] = [];
  for (const entry of view.transcript) {
    const parts = entry.parts;
    if (parts === null) {
      continue;
    }
    const message = entry.message;
    if (message.role === 'user') {
      sections.push(`## 用户\n\n${bodyText(parts)}`);
      continue;
    }
    if (message.role === 'compaction') {
      sections.push(`## 摘要\n\n${quote(bodyText(parts))}`);
      continue;
    }
    if (message.role !== 'assistant') {
      continue;
    }
    const text = bodyText(parts);
    const tools = parts
      .filter((part): part is ToolCallPart => part.kind === 'tool_call')
      .map((part) => toolQuote(part, results[part.callId]));
    const body = [text, ...tools].filter((piece) => piece.length > 0).join('\n\n');
    if (body.length > 0) {
      sections.push(`## 助手\n\n${body}`);
    }
  }
  const header = [
    `# ${view.session.title}`,
    [`- host：${host}`, `- workspace：${view.session.workspace}`].join('\n'),
  ];
  return `${[...header, ...sections].join('\n\n')}\n`;
}

/// 下载文件名：去掉路径分隔符与控制字符，空标题回退到 `session`。
export function markdownFileName(title: string): string {
  const name = title
    .replaceAll(/[/\\:*?"<>|]/g, '-')
    .replaceAll(/\s+/g, '-')
    .replaceAll(/-+/g, '-')
    .replaceAll(/^-|-$/g, '');
  return `${name.length === 0 ? 'session' : name}.md`;
}

function toolQuote(part: ToolCallPart, result: Parameters<typeof toolCardModel>[1]): string {
  const model = toolCardModel(part, result);
  const lines = [`> 工具调用 \`${part.name}\`（${toolStateLabel[model.state]}）`];
  if (part.arguments.length > 0) {
    lines.push(`> 参数：\`${part.arguments}\``);
  }
  return lines.join('\n');
}

function quote(text: string): string {
  return text
    .split('\n')
    .map((line) => `> ${line}`)
    .join('\n');
}
