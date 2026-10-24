import { bodyText } from './content';
import { transcriptNodes } from './messages';
import type { NodeState } from './node-state';

/// 一处命中。`index` 是该消息内的第几次出现。
export type SearchHit = {
  messageId: number;
  index: number;
  excerpt: string;
};

const EXCERPT_RADIUS = 20;

/// 在选中 session 的活动链上搜索已加载的正文。工具消息不参与。
export function searchTranscript(state: NodeState, sessionId: string, query: string): SearchHit[] {
  const needle = query.trim();
  if (needle.length === 0) {
    return [];
  }
  const hits: SearchHit[] = [];
  for (const node of transcriptNodes(state, sessionId)) {
    if (node.role === 'tool') {
      continue;
    }
    const parts = state.bodies[node.id];
    if (parts === undefined) {
      continue;
    }
    const text = bodyText(parts);
    const haystack = text.toLowerCase();
    const target = needle.toLowerCase();
    let occurrence = 0;
    let position = haystack.indexOf(target);
    while (position !== -1) {
      hits.push({
        messageId: node.id,
        index: occurrence,
        excerpt: excerptOf(text, position, needle.length),
      });
      occurrence += 1;
      position = haystack.indexOf(target, position + target.length);
    }
  }
  return hits;
}

/// 前后移动命中位置，在两端环绕。没有命中时返回 `-1`。
export function stepHit(current: number, delta: number, total: number): number {
  if (total <= 0) {
    return -1;
  }
  if (current < 0) {
    return delta < 0 ? total - 1 : 0;
  }
  return (current + delta + total) % total;
}

function excerptOf(text: string, position: number, length: number): string {
  const start = Math.max(0, position - EXCERPT_RADIUS);
  const end = Math.min(text.length, position + length + EXCERPT_RADIUS);
  const body = text.slice(start, end).replaceAll('\n', ' ');
  return `${start > 0 ? '…' : ''}${body}${end < text.length ? '…' : ''}`;
}
