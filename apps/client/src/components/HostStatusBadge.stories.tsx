import type { Meta, StoryObj } from '@storybook/react-vite';
import { HostStatusBadge } from './HostStatusBadge';

const meta = {
  title: 'Components/HostStatusBadge',
  component: HostStatusBadge,
  parameters: { layout: 'fullscreen', frame: 'centered' },
} satisfies Meta<typeof HostStatusBadge>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Ok: Story = { args: { status: 'ok' } };

export const Unreachable: Story = {
  args: { status: 'unreachable', detail: '连接 host 失败: 隧道已断开' },
};

export const Incompatible: Story = {
  args: { status: 'incompatible', detail: 'executor 构建 0 与 agent 要求的 1 不一致' },
};
