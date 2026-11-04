import type { Meta, StoryObj } from '@storybook/react-vite';
import { SessionListItem } from './SessionListItem';
import { WorkspaceGroup } from './WorkspaceGroup';
import { unreachableHost, workspaces } from '../fixtures/view';
import { toWorkspacePath } from '../view/ids';

const meta = {
  title: 'Components/WorkspaceGroup',
  component: WorkspaceGroup,
  parameters: { layout: 'fullscreen', frame: 'sidebar' },
} satisfies Meta<typeof WorkspaceGroup>;

export default meta;

type Story = StoryObj<typeof meta>;

const durable = workspaces[0];

export const Open: Story = {
  args: {
    host: 'studio',
    path: toWorkspacePath('/Users/zine/space/project/durable'),
    sessionCount: 3,
    children: (
      <>
        {durable?.sessions.map((session) => (
          <SessionListItem key={session.id} session={session} selected={false} />
        ))}
      </>
    ),
  },
};

export const Empty: Story = {
  args: {
    host: 'studio',
    path: toWorkspacePath('/Users/zine/space/project/empty-lab'),
    sessionCount: 0,
    children: <p>暂无 session</p>,
  },
};

export const Collapsed: Story = {
  args: {
    host: 'studio',
    path: toWorkspacePath('/Users/zine/space/project/agent-lab'),
    sessionCount: 1,
    defaultOpen: false,
    children: null,
  },
};

export const HostUnavailable: Story = {
  args: {
    host: 'studio',
    path: toWorkspacePath('/Users/zine/space/project/studio-lab'),
    sessionCount: 0,
    writesDisabled: true,
    children: <p>{unreachableHost.notice}</p>,
  },
};

export const LongPath: Story = {
  args: {
    host: 'studio',
    path: toWorkspacePath('/Users/zine/space/project/clients/acme/platform/services/edge-gateway'),
    sessionCount: 2,
    children: null,
  },
};
