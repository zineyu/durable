import type { Meta, StoryObj } from '@storybook/react-vite';
import { SessionListItem } from './SessionListItem';
import { toSessionId } from '../view/ids';
import type { SessionSummary } from '../view/types';

const idle: SessionSummary = {
  id: toSessionId('s-idle'),
  title: '类型生成一致性检查',
  state: 'idle',
  queuedCount: 0,
};

const meta = {
  title: 'Components/SessionListItem',
  component: SessionListItem,
  parameters: { layout: 'fullscreen', frame: 'sidebar' },
} satisfies Meta<typeof SessionListItem>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Idle: Story = { args: { session: idle, selected: false } };

export const Selected: Story = { args: { session: idle, selected: true } };

export const MenuOpen: Story = {
  args: { session: idle, selected: true, defaultMenuOpen: true },
};

export const Running: Story = {
  args: {
    session: {
      id: toSessionId('s-running'),
      title: '双二进制骨架与工程基线',
      state: 'running',
      queuedCount: 0,
    },
    selected: false,
  },
};

export const QueuedWithBacklog: Story = {
  args: {
    session: {
      id: toSessionId('s-queued'),
      title: 'UI 反代与静态资源内嵌',
      state: 'queued',
      queuedCount: 3,
    },
    selected: false,
  },
};

export const LongTitle: Story = {
  args: {
    session: {
      id: toSessionId('s-long'),
      title: '把 ui 的静态资源改为内嵌并确认 SSE 在反代中不被缓冲且断线可续传',
      state: 'idle',
      queuedCount: 0,
    },
    selected: false,
  },
};
