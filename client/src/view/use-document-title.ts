import { useEffect } from 'react';
import { documentTitle } from './title';
import type { SessionView } from './types';

/// 页面标题跟随选中 session 的派生视图。
export function useDocumentTitle(view: SessionView | null): void {
  useEffect(() => {
    document.title =
      view === null
        ? documentTitle(null)
        : documentTitle({
            host: view.session.host,
            title: view.summary.title,
            state: view.summary.state,
            lastRun: view.lastRun,
          });
  }, [view]);
}
