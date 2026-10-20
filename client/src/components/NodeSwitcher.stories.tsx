import type { Meta, StoryObj } from '@storybook/react-vite';
import { NodeSwitcher } from './NodeSwitcher';
import { incompatibleNode, nodes } from '../fixtures/view';

const meta = {
  title: 'Components/NodeSwitcher',
  component: NodeSwitcher,
  parameters: { layout: 'fullscreen', frame: 'sidebar' },
} satisfies Meta<typeof NodeSwitcher>;

export default meta;

type Story = StoryObj<typeof meta>;

export const SelectedReachable: Story = {
  args: { nodes, selected: 'macbook-air-01' },
};

export const SelectedUnreachable: Story = {
  args: { nodes, selected: 'studio' },
};

export const WithIncompatible: Story = {
  args: { nodes: [...nodes, incompatibleNode], selected: 'legacy-lab' },
};

export const UnknownSelection: Story = {
  args: { nodes, selected: 'retired-studio' },
};

export const MenuOpen: Story = {
  args: { nodes, selected: 'studio', defaultMenuOpen: true },
};

export const Empty: Story = {
  args: { nodes: [], selected: null },
};
