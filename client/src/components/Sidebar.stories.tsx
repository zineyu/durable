import type { Meta, StoryObj } from '@storybook/react-vite';
import { Sidebar } from './Sidebar';
import { toSessionId } from '../view/ids';
import {
  modelName,
  nodes,
  unreachableNode,
  workspaceWithoutSessions,
  workspaces,
} from '../fixtures/view';

const meta = {
  title: 'Components/Sidebar',
  component: Sidebar,
  parameters: { layout: 'fullscreen', frame: 'sidebar' },
} satisfies Meta<typeof Sidebar>;

export default meta;

type Story = StoryObj<typeof meta>;

const base = {
  nodes,
  selectedNode: 'macbook-air-01',
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
    selectedSessionId: null,
  },
};

export const WorkspaceWithoutSessions: Story = {
  args: {
    ...base,
    workspaces: workspaceWithoutSessions,
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

export const UnreachableNode: Story = {
  args: {
    ...base,
    selectedNode: unreachableNode.name,
    workspaces: [],
    selectedSessionId: null,
    notice: unreachableNode.detail,
    commandsDisabled: true,
  },
};

export const UnknownNode: Story = {
  args: {
    ...base,
    selectedNode: 'retired-studio',
    workspaces: [],
    selectedSessionId: null,
    notice: '未知节点，节点表里没有这个名字',
    commandsDisabled: true,
  },
};

export const NoNodes: Story = {
  args: {
    ...base,
    nodes: [],
    selectedNode: null,
    workspaces: [],
    selectedSessionId: null,
  },
};
