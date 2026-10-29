import { describe, expect, it } from 'vitest';

import type { ContentPart } from '../api/generated/ContentPart';
import type { MessageSkeleton } from '../api/generated/MessageSkeleton';
import type { SessionMetadata } from '../api/generated/SessionMetadata';
import { sessionView } from './derived';
import { markdownFileName, sessionMarkdown } from './session-markdown';
import { emptyClientState } from './state';
import type { ClientState } from './state';
import type { SessionView } from './types';

const session: SessionMetadata = {
  id: 's-1',
  host: 'local',
  workspace: '/work/a',
  title: '迁移与恢复',
  busy: false,
  queueLength: 0,
  updatedAt: 10,
};

function message(id: number, parentId: number | null, role: MessageSkeleton['role']): MessageSkeleton {
  return { id, parentId, role, interrupted: false, runId: null, createdAt: id, parts: [] };
}

function text(value: string): ContentPart {
  return { kind: 'text', text: value, providerMeta: null };
}

function state(bodies: Record<number, ContentPart[]>): ClientState {
  return {
    ...emptyClientState,
    sessions: [session],
    trees: {
      's-1': {
        activeLeaf: 4,
        nodes: [
          message(1, null, 'user'),
          message(2, 1, 'assistant'),
          message(3, 2, 'tool'),
          message(4, 3, 'assistant'),
        ],
        queue: [],
        runs: [],
      },
    },
    bodies,
  };
}

function viewOf(value: ClientState): SessionView {
  const view = sessionView(value, 's-1');
  if (view === null) {
    throw new Error('缺少 session 视图');
  }
  return view;
}

describe('Markdown 导出', () => {
  it('按消息顺序输出标题、用户与助手正文', () => {
    const markdown = sessionMarkdown(
      viewOf(
        state({ 1: [text('把迁移补上。')], 2: [text('先读 README。')], 4: [text('迁移已完成。')] }),
      ),
      'macbook-air-01',
    );
    expect(markdown).toContain('# 迁移与恢复');
    expect(markdown).toContain('- host：macbook-air-01');
    expect(markdown).toContain('- workspace：/work/a');
    expect(markdown.indexOf('## 用户')).toBeLessThan(markdown.indexOf('## 助手'));
    expect(markdown).toContain('把迁移补上。');
    expect(markdown).toContain('迁移已完成。');
  });

  it('工具调用折叠为引用块并带上结局', () => {
    const markdown = sessionMarkdown(
      viewOf(
        state({
        1: [text('读 README。')],
        2: [
          text('先看文件。'),
          {
            kind: 'tool_call',
            callId: 'call-1',
            name: 'read_file',
            arguments: '{"path":"README.md"}',
            providerMeta: null,
          },
        ],
        3: [
          {
            kind: 'tool_result',
            callId: 'call-1',
            state: { status: 'finished', outcome: { status: 'succeeded', content: 'ok' } },
            providerMeta: null,
          },
        ],
        4: [text('完成。')],
        }),
      ),
      'macbook-air-01',
    );
    expect(markdown).toContain('> 工具调用 `read_file`（成功）');
    expect(markdown).toContain('> 参数：`{"path":"README.md"}`');
    expect(markdown).not.toContain('## 工具');
  });

  it('压缩消息渲染为引用块', () => {
    const compacted: ClientState = {
      ...emptyClientState,
      sessions: [session],
      trees: {
        's-1': {
          activeLeaf: 4,
          nodes: [message(4, null, 'compaction')],
          queue: [],
          runs: [],
        },
      },
      bodies: { 4: [text('之前的对话摘要。')] },
    };
    const markdown = sessionMarkdown(viewOf(compacted), 'macbook-air-01');
    expect(markdown).toContain('## 摘要');
    expect(markdown).toContain('> 之前的对话摘要。');
  });

  it('文件名去掉路径分隔符与空白', () => {
    expect(markdownFileName('迁移/恢复 与 重试')).toBe('迁移-恢复-与-重试.md');
    expect(markdownFileName('   ')).toBe('session.md');
  });
});
