import type { Meta, StoryObj } from '@storybook/react-vite';
import { SessionListItem } from './SessionListItem';
import { toSessionId } from '../view/ids';
import type { SessionSummary } from '../view/types';

const updatedAt = new Date(2026, 9, 19, 15, 58).getTime();

const idle: SessionSummary = {
  id: toSessionId('s-idle'),
  title: '类型生成一致性检查',
  state: 'idle',
  queuedCount: 0,
  updatedAt,
  unread: false,
  outcome: null,
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

export const Renaming: Story = {
  args: { session: idle, selected: true, view: 'renaming' },
};

export const DeleteConfirm: Story = {
  args: { session: idle, selected: true, view: 'deleting' },
};

export const Running: Story = {
  args: {
    session: {
      id: toSessionId('s-running'),
      title: '双二进制骨架与工程基线',
      state: 'running',
      queuedCount: 0,
      updatedAt,
      unread: false,
      outcome: null,
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
      updatedAt,
      unread: false,
      outcome: null,
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
      updatedAt,
      unread: false,
      outcome: null,
    },
    selected: false,
  },
};

export const UnreadCompleted: Story = {
  args: {
    session: { ...idle, unread: true, outcome: 'succeeded' },
    selected: false,
  },
};

export const UnreadFailed: Story = {
  args: {
    session: { ...idle, id: toSessionId('s-failed'), title: '实现迁移并在重启后恢复队列', unread: true, outcome: 'failed' },
    selected: false,
  },
};
