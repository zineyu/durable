import type { Meta, StoryObj } from '@storybook/react-vite';
import { UserMessage } from './UserMessage';
import { longQueueItem } from '../fixtures/view';

const meta = {
  title: 'Components/UserMessage',
  component: UserMessage,
  parameters: { layout: 'fullscreen', frame: 'content' },
} satisfies Meta<typeof UserMessage>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Default: Story = {
  args: { text: '把 ui 的静态资源改为内嵌，并确认 SSE 不被缓冲。' },
};

export const LongText: Story = {
  args: { text: longQueueItem },
};

export const Multiline: Story = {
  args: {
    text: '补一个 health 端点的回环地址测试。\n测试需要覆盖：非回环地址被拒绝、无效地址被拒绝。',
  },
};

export const WithBranches: Story = {
  args: {
    text: '把 ui 的静态资源改为内嵌。',
    branch: { index: 2, total: 3, onPrev: () => undefined, onNext: () => undefined },
  },
};
