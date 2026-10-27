import { describe, expect, it } from 'vitest';

import type { ContentPart } from '../api/generated/ContentPart';
import type { MessageSkeleton } from '../api/generated/MessageSkeleton';
import type { SessionMetadata } from '../api/generated/SessionMetadata';
import { sessionView } from './derived';
import { stepHit, searchTranscript } from './search';
import type { NodeState } from './node-state';
import type { SessionView } from './types';

const session: SessionMetadata = {
  id: 's-1',
  workspace: '/work/a',
  title: '迁移',
  busy: false,
  queueLength: 0,
  updatedAt: 10,
};

function message(id: number, parentId: number | null, role: MessageSkeleton['role']): MessageSkeleton {
  return { id, parentId, role, interrupted: false, runId: null, createdAt: id, parts: [] };
}

function text(value: string): ContentPart[] {
  return [{ kind: 'text', text: value, providerMeta: null }];
}

function state(bodies: Record<number, ContentPart[]>): NodeState {
  return {
    modelName: 'm',
    workspaces: [],
    sessions: [session],
    trees: {
      's-1': {
        activeLeaf: 3,
        nodes: [message(1, null, 'user'), message(2, 1, 'assistant'), message(3, 2, 'tool')],
        queue: [],
        runs: [],
      },
    },
    retries: {},
    bodies,
    bodyRequests: [],
    bodyFailures: [],
    placeholders: {},
    lastSeenAt: {},
    lastEventId: null,
    applied: [],
  };
}

function viewOf(value: NodeState): SessionView {
  const view = sessionView(value, 's-1');
  if (view === null) {
    throw new Error('缺少 session 视图');
  }
  return view;
}

describe('session 内搜索', () => {
  it('在 transcript 的正文里按顺序返回全部命中', () => {
    const hits = searchTranscript(
      viewOf(state({ 1: text('先做迁移，再看迁移的恢复。'), 2: text('迁移已经完成。'), 3: text('迁移') })),
      '迁移',
    );
    expect(hits.map((hit) => [hit.messageId, hit.index])).toEqual([
      [1, 0],
      [1, 1],
      [2, 0],
    ]);
  });

  it('忽略大小写，并在命中处截取上下文', () => {
    const long = `${'前'.repeat(40)}Needle${'后'.repeat(40)}`;
    const hits = searchTranscript(viewOf(state({ 1: text(long) })), 'needle');
    expect(hits).toHaveLength(1);
    expect(hits[0]?.excerpt).toContain('Needle');
    expect(hits[0]?.excerpt.startsWith('…')).toBe(true);
    expect(hits[0]?.excerpt.endsWith('…')).toBe(true);
  });

  it('空查询没有命中，工具消息不参与搜索', () => {
    const view = viewOf(state({ 1: text('迁移'), 3: text('迁移') }));
    expect(searchTranscript(view, '  ')).toEqual([]);
    expect(searchTranscript(view, '迁移')).toHaveLength(1);
  });

  it('正文未加载的消息不参与搜索', () => {
    expect(searchTranscript(viewOf(state({})), '迁移')).toEqual([]);
  });

  it('压缩消息的摘要也参与搜索', () => {
    const compacted: NodeState = {
      ...state({ 5: text('此前的迁移摘要') }),
      trees: {
        's-1': {
          activeLeaf: 5,
          nodes: [message(5, null, 'compaction')],
          queue: [],
          runs: [],
        },
      },
    };
    expect(searchTranscript(viewOf(compacted), '迁移')).toHaveLength(1);
  });
});

describe('命中跳转', () => {
  it('前后移动并在两端环绕', () => {
    expect(stepHit(-1, 1, 3)).toBe(0);
    expect(stepHit(0, 1, 3)).toBe(1);
    expect(stepHit(2, 1, 3)).toBe(0);
    expect(stepHit(0, -1, 3)).toBe(2);
  });

  it('没有命中时停在 -1', () => {
    expect(stepHit(-1, 1, 0)).toBe(-1);
    expect(stepHit(-1, -1, 0)).toBe(-1);
  });
});
