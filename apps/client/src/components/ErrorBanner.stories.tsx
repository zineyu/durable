import type { Meta, StoryObj } from '@storybook/react-vite';
import { ErrorBanner } from './ErrorBanner';

const meta = {
  title: 'Components/ErrorBanner',
  component: ErrorBanner,
  parameters: { layout: 'fullscreen' },
} satisfies Meta<typeof ErrorBanner>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Default: Story = {
  args: {
    code: 'llm.upstream_error',
    message: '模型上游返回 502，当前 run 已结束，队列将继续执行。',
  },
};

export const LongMessage: Story = {
  args: {
    code: 'context.limit_exceeded',
    message:
      'transcript 已超过 200000 字符上限。当前 run 已结束。请新建 session 以继续，v1 不做上下文压缩。',
  },
};
