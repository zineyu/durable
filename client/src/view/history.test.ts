import { describe, expect, it } from 'vitest';

import type { ContentPart } from '../api/generated/ContentPart';
import type { MessageSkeleton } from '../api/generated/MessageSkeleton';
import type { SessionMetadata } from '../api/generated/SessionMetadata';
import { sessionHistory } from './history';
import { emptyNodeState } from './node-state';
import type { NodeState } from './node-state';

const session: SessionMetadata = {
  id: 's-1',
  workspace: '/work/a',
  title: '标题',
  busy: false,
  queueLength: 0,
  updatedAt: 10,
};

function message(id: number, parentId: number | null, role: MessageSkeleton['role']): MessageSkeleton {
  return { id, parentId, role, runId: null, createdAt: id, parts: [] };
}

function text(value: string): ContentPart[] {
  return [{ kind: 'text', text: value, providerMeta: null }];
}

describe('已发送消息历史', () => {
  it('按顺序返回活动链上的用户消息正文', () => {
    const shard: NodeState = {
      ...emptyNodeState,
      sessions: [session],
      trees: {
        's-1': {
          activeLeaf: 3,
          nodes: [
            message(1, null, 'user'),
            message(2, 1, 'assistant'),
            message(3, 2, 'user'),
          ],
          queue: [],
          runs: [],
        },
      },
      bodies: { 1: text('第一条'), 2: text('回答'), 3: text('第二条') },
    };
    expect(sessionHistory(shard, 's-1')).toEqual(['第一条', '第二条']);
  });

  it('正文未加载或为空的消息不进入历史', () => {
    const shard: NodeState = {
      ...emptyNodeState,
      sessions: [session],
      trees: {
        's-1': {
          activeLeaf: 2,
          nodes: [message(1, null, 'user'), message(2, 1, 'user')],
          queue: [],
          runs: [],
        },
      },
      bodies: { 1: text('  ') },
    };
    expect(sessionHistory(shard, 's-1')).toEqual([]);
  });
});
