import type { Meta, StoryObj } from '@storybook/react-vite';
import { StatusDot } from './StatusDot';

const meta = {
  title: 'Components/StatusDot',
  component: StatusDot,
  parameters: { layout: 'fullscreen', frame: 'centered' },
} satisfies Meta<typeof StatusDot>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Idle: Story = { args: { tone: 'idle', label: '空闲' } };

export const Running: Story = { args: { tone: 'running', label: '执行中' } };

export const Queued: Story = { args: { tone: 'queued', label: '排队中' } };

export const Success: Story = { args: { tone: 'success', label: '成功' } };

export const Warn: Story = { args: { tone: 'warn', label: '警告' } };

export const Danger: Story = { args: { tone: 'danger', label: '失败' } };
