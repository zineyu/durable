import type { Meta, StoryObj } from '@storybook/react-vite';
import { Sidebar } from './Sidebar';
import { toSessionId } from '../view/ids';
import { modelName, workspaceWithoutSessions, workspaces } from '../fixtures/view';

const meta = {
  title: 'Components/Sidebar',
  component: Sidebar,
  parameters: { layout: 'fullscreen', frame: 'sidebar' },
} satisfies Meta<typeof Sidebar>;

export default meta;

type Story = StoryObj<typeof meta>;

export const WithWorkspaces: Story = {
  args: {
    modelName,
    workspaces,
    selectedSessionId: toSessionId('s-2'),
  },
};

export const Empty: Story = {
  args: {
    modelName,
    workspaces: [],
    selectedSessionId: null,
  },
};

export const WorkspaceWithoutSessions: Story = {
  args: {
    modelName,
    workspaces: workspaceWithoutSessions,
    selectedSessionId: null,
  },
};
