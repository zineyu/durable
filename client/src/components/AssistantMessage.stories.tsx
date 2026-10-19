import type { Meta, StoryObj } from '@storybook/react-vite';
import { AssistantMessage } from './AssistantMessage';
import { CodeBlock } from './CodeBlock';
import { codeSample } from '../fixtures/view';

const meta = {
  title: 'Components/AssistantMessage',
  component: AssistantMessage,
  parameters: { layout: 'fullscreen', frame: 'content' },
} satisfies Meta<typeof AssistantMessage>;

export default meta;

type Story = StoryObj<typeof meta>;

const body = (
  <>
    <p>health 端点已经就位。类型从 Rust 定义导出，前端只消费生成结果。</p>
    <CodeBlock code={codeSample} language="rust" />
  </>
);

export const Complete: Story = {
  args: { state: 'complete', children: body },
};

export const Streaming: Story = {
  args: {
    state: 'streaming',
    children: <p>health 端点已经就位。类型从 Rust 定义导出，前端只消费生成</p>,
  },
};

export const Cancelled: Story = {
  args: {
    state: 'cancelled',
    children: <p>health 端点已经就位。类型从 Rust 定义</p>,
    note: '已取消，收到的部分输出保留在 transcript 中。',
  },
};

export const Failed: Story = {
  args: {
    state: 'failed',
    children: <p>health 端点已经就位。</p>,
    note: '模型上游错误：连接被拒绝。队列将继续执行下一项。',
  },
};

export const WithBranches: Story = {
  args: {
    state: 'complete',
    children: body,
    branch: { index: 2, total: 3 },
  },
};

export const FailedWithBranches: Story = {
  args: {
    state: 'failed',
    children: <p>health 端点已经就位。</p>,
    note: '模型上游错误：连接被拒绝。队列将继续执行下一项。',
    branch: { index: 1, total: 3 },
  },
};

export const Retryable: Story = {
  args: { state: 'complete', children: body, onRetry: () => undefined },
};

export const WithBranchesAndRetry: Story = {
  args: {
    state: 'complete',
    children: body,
    branch: { index: 2, total: 3, onPrev: () => undefined, onNext: () => undefined },
    onRetry: () => undefined,
  },
};
