import type { Meta, StoryObj } from '@storybook/react-vite';
import { EmptyState } from './EmptyState';

const meta = {
  title: 'Components/EmptyState',
  component: EmptyState,
  parameters: { layout: 'fullscreen', frame: 'centered' },
} satisfies Meta<typeof EmptyState>;

export default meta;

type Story = StoryObj<typeof meta>;

export const NoSession: Story = {
  args: {
    title: '选择一个 session',
    description: '从左侧选择 session，或者注册新的 workspace。',
    actionLabel: '注册 workspace',
  },
};

export const NoWorkspace: Story = {
  args: {
    title: '还没有 workspace',
    description: '注册本机上的目录作为 workspace，然后创建 session。',
    actionLabel: '注册 workspace',
  },
};

export const NoDescription: Story = {
  args: {
    title: '队列为空',
    description: '新消息会先进入队列，再按顺序执行。',
  },
};
