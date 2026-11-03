import type { Meta, StoryObj } from '@storybook/react-vite';
import { Sidebar } from './Sidebar';
import { toSessionId } from '../view/ids';
import {
  hosts,
  modelName,
  unknownHostWorkspaces,
  unreachableWorkspaces,
  workspaceWithoutSessions,
  workspaces,
} from '../fixtures/view';
import type { RunningEntry } from '../view/types';

const meta = {
  title: 'Components/Sidebar',
  component: Sidebar,
  parameters: { layout: 'fullscreen', frame: 'sidebar' },
} satisfies Meta<typeof Sidebar>;

export default meta;

type Story = StoryObj<typeof meta>;

const sessions = workspaces.flatMap((workspace) => workspace.sessions);

const base = {
  hosts,
  selectedHost: 'macbook-air-01',
  sessions,
  modelName,
  workspaces,
};

export const WithWorkspaces: Story = {
  args: {
    ...base,
    selectedSessionId: toSessionId('s-2'),
  },
};

export const Empty: Story = {
  args: {
    ...base,
    workspaces: [],
    sessions: [],
    selectedSessionId: null,
  },
};

export const WorkspaceWithoutSessions: Story = {
  args: {
    ...base,
    workspaces: workspaceWithoutSessions,
    sessions: [],
    selectedSessionId: null,
  },
};

export const Renaming: Story = {
  args: {
    ...base,
    selectedSessionId: toSessionId('s-2'),
    editing: { sessionId: toSessionId('s-2'), view: 'renaming' },
  },
};

export const DeleteConfirm: Story = {
  args: {
    ...base,
    selectedSessionId: toSessionId('s-2'),
    editing: { sessionId: toSessionId('s-2'), view: 'deleting' },
  },
};

export const UnreachableHost: Story = {
  args: {
    ...base,
    selectedHost: 'studio',
    workspaces: unreachableWorkspaces,
    sessions: [],
    selectedSessionId: null,
    registerDisabled: true,
  },
};

export const UnknownHost: Story = {
  args: {
    ...base,
    selectedHost: 'retired-studio',
    workspaces: unknownHostWorkspaces,
    sessions: [],
    selectedSessionId: null,
    registerDisabled: true,
  },
};

export const NoHosts: Story = {
  args: {
    ...base,
    hosts: [],
    selectedHost: null,
    workspaces: [],
    sessions: [],
    selectedSessionId: null,
  },
};

const running: RunningEntry[] = [
  {
    host: 'macbook-air-01',
    sessionId: toSessionId('s-1'),
    title: '双二进制骨架与工程基线',
    queueLength: 2,
    startedAt: 2,
  },
  {
    host: 'macbook-air-02',
    sessionId: toSessionId('s-4'),
    title: 'SQLite 迁移与恢复',
    queueLength: 0,
    startedAt: 1,
  },
];

export const WithActivity: Story = {
  args: {
    ...base,
    selectedSessionId: toSessionId('s-2'),
    running,
  },
};

export const UnreadMarks: Story = {
  args: {
    ...base,
    selectedSessionId: toSessionId('s-3'),
    workspaces: workspaces.map((workspace) => ({
      ...workspace,
      sessions: workspace.sessions.map((session) =>
        session.id === toSessionId('s-2')
          ? { ...session, unread: true, outcome: 'failed' as const }
          : session.id === toSessionId('s-4')
            ? { ...session, unread: true, outcome: 'succeeded' as const }
            : session,
      ),
    })),
  },
};
