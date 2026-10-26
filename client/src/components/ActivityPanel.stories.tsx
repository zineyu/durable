import type { Meta, StoryObj } from '@storybook/react-vite';
import { ActivityPanel } from './ActivityPanel';
import { toSessionId } from '../view/ids';
import type { RunningEntry } from '../view/types';

const entries: RunningEntry[] = [
  {
    node: 'macbook-air-01',
    sessionId: toSessionId('s-1'),
    title: '双二进制骨架与工程基线',
    queueLength: 0,
    startedAt: 1,
  },
  {
    node: 'macbook-air-02',
    sessionId: toSessionId('s-4'),
    title: 'SQLite 迁移与恢复',
    queueLength: 2,
    startedAt: 2,
  },
  {
    node: 'studio',
    sessionId: toSessionId('s-5'),
    title: '把 ui 的静态资源改为内嵌并确认 SSE 在反代中不被缓冲',
    queueLength: 0,
    startedAt: null,
  },
];

const meta = {
  title: 'Components/ActivityPanel',
  component: ActivityPanel,
  parameters: { layout: 'fullscreen', frame: 'sidebar' },
} satisfies Meta<typeof ActivityPanel>;

export default meta;

type Story = StoryObj<typeof meta>;

export const MultipleNodes: Story = { args: { entries } };

export const Empty: Story = { args: { entries: [] } };
