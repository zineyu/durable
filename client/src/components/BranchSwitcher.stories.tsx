import type { Meta, StoryObj } from '@storybook/react-vite';
import { BranchSwitcher } from './BranchSwitcher';

const meta = {
  title: 'Components/BranchSwitcher',
  component: BranchSwitcher,
  parameters: { layout: 'fullscreen', frame: 'centered' },
} satisfies Meta<typeof BranchSwitcher>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Middle: Story = {
  args: { index: 2, total: 3, onPrev: () => undefined, onNext: () => undefined },
};

export const Locked: Story = { args: { index: 2, total: 3 } };

export const First: Story = { args: { index: 1, total: 3 } };

export const Only: Story = { args: { index: 1, total: 1 } };

export const ManyBranches: Story = { args: { index: 7, total: 12 } };
