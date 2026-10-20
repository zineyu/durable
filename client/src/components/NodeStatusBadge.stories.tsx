import type { Meta, StoryObj } from '@storybook/react-vite';
import { NodeStatusBadge } from './NodeStatusBadge';

const meta = {
  title: 'Components/NodeStatusBadge',
  component: NodeStatusBadge,
  parameters: { layout: 'fullscreen', frame: 'centered' },
} satisfies Meta<typeof NodeStatusBadge>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Ok: Story = { args: { status: 'ok' } };

export const Unreachable: Story = {
  args: { status: 'unreachable', detail: '连接节点失败: 隧道已断开' },
};

export const Incompatible: Story = {
  args: { status: 'incompatible', detail: 'agent 协议版本 0 与 hub 要求的 1 不一致' },
};
