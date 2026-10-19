import { useCallback, useEffect, useState } from 'react';
import { AppShell } from './components/AppShell';
import { EmptyState } from './components/EmptyState';
import { Sidebar } from './components/Sidebar';
import { modelName, workspaces } from './fixtures/view';
import type { SessionId } from './view/ids';
import { readSessionFromHash, writeSessionToHash } from './view/session-hash';

export function App() {
  const [selectedSessionId, setSelectedSessionId] = useState<SessionId | null>(() =>
    readSessionFromHash(window.location.hash),
  );

  useEffect(() => {
    const handleHashChange = () => {
      setSelectedSessionId(readSessionFromHash(window.location.hash));
    };
    window.addEventListener('hashchange', handleHashChange);
    return () => window.removeEventListener('hashchange', handleHashChange);
  }, []);

  const selectSession = useCallback((id: SessionId) => {
    window.location.hash = writeSessionToHash(id);
  }, []);

  return (
    <AppShell
      sidebar={
        <Sidebar
          modelName={modelName}
          workspaces={workspaces}
          selectedSessionId={selectedSessionId}
          onSelectSession={selectSession}
        />
      }
    >
      <EmptyState
        title="选择一个 session"
        description="从左侧选择 session，或者注册新的 workspace。"
        actionLabel="注册 workspace"
      />
    </AppShell>
  );
}
