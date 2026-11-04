import type { Meta, StoryObj } from '@storybook/react-vite';
import { AppShell } from './AppShell';
import { EmptyState } from './EmptyState';
import { Sidebar } from './Sidebar';
import { toSessionId } from '../view/ids';
import { hosts, modelName, workspaces } from '../fixtures/view';

const meta = {
  title: 'Components/AppShell',
  component: AppShell,
  parameters: { layout: 'fullscreen' },
} satisfies Meta<typeof AppShell>;

export default meta;

type Story = StoryObj<typeof meta>;

export const WithSidebar: Story = {
  args: {
    sidebar: (
      <Sidebar
        hosts={hosts}
        selectedHost="macbook-air-01"
        sessions={workspaces.flatMap((workspace) => workspace.sessions)}
        modelName={modelName}
        workspaces={workspaces}
        selectedSessionId={toSessionId('s-2')}
      />
    ),
    children: (
      <EmptyState
        title="选择一个 session"
        description="从左侧选择 session，或者注册新的 workspace。"
        actionLabel="注册 workspace"
      />
    ),
  },
};
