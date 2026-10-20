import type { Meta, StoryObj } from '@storybook/react-vite';
import { MainHeader } from './MainHeader';
import { toWorkspacePath } from '../view/ids';

const meta = {
  title: 'Components/MainHeader',
  component: MainHeader,
  parameters: { layout: 'fullscreen' },
} satisfies Meta<typeof MainHeader>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Idle: Story = {
  args: {
    node: 'macbook-air-01',
    nodeStatus: 'ok',
    title: '类型生成一致性检查',
    workspacePath: toWorkspacePath('/Users/zine/space/project/durable'),
    state: 'idle',
    connection: 'connected',
  },
};

export const Running: Story = {
  args: {
    node: 'macbook-air-01',
    nodeStatus: 'ok',
    title: '双二进制骨架与工程基线',
    workspacePath: toWorkspacePath('/Users/zine/space/project/durable'),
    state: 'running',
    connection: 'connected',
  },
};

export const Reconnecting: Story = {
  args: {
    node: 'macbook-air-01',
    nodeStatus: 'ok',
    title: 'SQLite 迁移与恢复',
    workspacePath: toWorkspacePath('/Users/zine/space/project/agent-lab'),
    state: 'queued',
    connection: 'reconnecting',
  },
};

export const LongTitle: Story = {
  args: {
    node: 'macbook-air-01',
    nodeStatus: 'ok',
    title: '把 ui 的静态资源改为内嵌并确认 SSE 在反代中不被缓冲且断线可续传',
    workspacePath: toWorkspacePath('/Users/zine/space/project/clients/acme/platform/services/edge-gateway'),
    state: 'idle',
    connection: 'connected',
  },
};
