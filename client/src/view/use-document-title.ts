import { useEffect } from 'react';
import type { NodeName } from '../api/generated/NodeName';
import { documentTitle } from './title';
import type { SessionView } from './types';

/// 页面标题跟随选中 session 的派生视图。
export function useDocumentTitle(node: NodeName | null, view: SessionView | null): void {
  useEffect(() => {
    document.title =
      node === null || view === null
        ? documentTitle(null)
        : documentTitle({
            node,
            title: view.summary.title,
            state: view.summary.state,
            lastRun: view.lastRun,
          });
  }, [node, view]);
}
