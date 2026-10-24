import { bodyOf } from './bodies';
import { bodyText } from './content';
import { transcriptNodes } from './messages';
import type { NodeState } from './node-state';

/// 当前活动链上已发送的用户消息正文，按时间顺序。输入框的 `↑` 回溯用它。
export function sessionHistory(state: NodeState, sessionId: string): string[] {
  const history: string[] = [];
  for (const message of transcriptNodes(state, sessionId)) {
    if (message.role !== 'user') {
      continue;
    }
    const parts = bodyOf(state, message.id);
    if (parts === null) {
      continue;
    }
    const text = bodyText(parts);
    if (text.trim().length > 0) {
      history.push(text);
    }
  }
  return history;
}
