import { AppShell } from './components/AppShell';
import { EmptyState } from './components/EmptyState';
import { Sidebar } from './components/Sidebar';
import { modelName, workspaces } from './fixtures/view';

export function App() {
  return (
    <AppShell
      sidebar={<Sidebar modelName={modelName} workspaces={workspaces} selectedSessionId={null} />}
    >
      <EmptyState
        title="选择一个 session"
        description="从左侧选择 session，或者注册新的 workspace。"
        actionLabel="注册 workspace"
      />
    </AppShell>
  );
}
