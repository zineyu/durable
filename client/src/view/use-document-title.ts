import { useEffect } from 'react';
import type { NodeName } from '../api/generated/NodeName';
import type { SessionMetadata } from '../api/generated/SessionMetadata';
import { lastRunStatus } from './activity';
import { sessionStateOf } from './labels';
import type { NodeState } from './node-state';
import { documentTitle } from './title';

/// 页面标题跟随选中 session 的运行状态。
export function useDocumentTitle(
  node: NodeName | null,
  session: SessionMetadata | undefined,
  shard: NodeState | undefined,
): void {
  useEffect(() => {
    document.title =
      node === null || session === undefined || shard === undefined
        ? documentTitle(null)
        : documentTitle({
            node,
            title: session.title,
            state: sessionStateOf(session.busy, session.queueLength),
            lastRun: lastRunStatus(shard, session.id),
          });
  }, [node, session, shard]);
}
