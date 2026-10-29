import type { Meta, StoryObj } from '@storybook/react-vite';
import { HostSwitcher } from './HostSwitcher';
import { hosts, incompatibleHost, workspaces } from '../fixtures/view';
import { toSessionId } from '../view/ids';

const sessions = workspaces.flatMap((workspace) => workspace.sessions);

const meta = {
  title: 'Components/HostSwitcher',
  component: HostSwitcher,
  parameters: { layout: 'fullscreen', frame: 'sidebar' },
} satisfies Meta<typeof HostSwitcher>;

export default meta;

type Story = StoryObj<typeof meta>;

export const SelectedReachable: Story = {
  args: {
    hosts,
    sessions,
    selectedHost: 'macbook-air-01',
    selectedSessionId: toSessionId('s-4'),
    defaultMenuOpen: true,
  },
};

export const SelectedUnreachable: Story = {
  args: { hosts, sessions, selectedHost: 'studio', defaultMenuOpen: true },
};

export const WithIncompatible: Story = {
  args: {
    hosts: [...hosts, incompatibleHost],
    sessions,
    selectedHost: 'legacy-lab',
    defaultMenuOpen: true,
  },
};

export const Collapsed: Story = {
  args: { hosts, sessions, selectedHost: 'macbook-air-01' },
};

export const Empty: Story = {
  args: { hosts: [], sessions: [], selectedHost: null, defaultMenuOpen: true },
};
