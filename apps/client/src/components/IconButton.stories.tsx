import type { Meta, StoryObj } from '@storybook/react-vite';
import { IconButton } from './IconButton';

const meta = {
  title: 'Components/IconButton',
  component: IconButton,
  parameters: { layout: 'fullscreen', frame: 'centered' },
} satisfies Meta<typeof IconButton>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Small: Story = { args: { icon: 'pencil', label: '编辑消息' } };

export const Medium: Story = { args: { icon: 'pencil', label: '重命名', size: 'md' } };

export const Danger: Story = { args: { icon: 'trash', label: '删除', tone: 'danger', size: 'md' } };

export const Disabled: Story = {
  args: { icon: 'trash', label: '删除', tone: 'danger', size: 'md', disabled: true },
};
