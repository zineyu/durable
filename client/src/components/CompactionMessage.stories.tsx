import type { Meta, StoryObj } from '@storybook/react-vite';
import type { ContentPart } from '../api/generated/ContentPart';
import type { PartSkeleton } from '../api/generated/PartSkeleton';
import { CompactionMessage } from './CompactionMessage';

const meta = {
  title: 'Components/CompactionMessage',
  component: CompactionMessage,
  parameters: { layout: 'fullscreen', frame: 'content' },
} satisfies Meta<typeof CompactionMessage>;

export default meta;

type Story = StoryObj<typeof meta>;

const summary: ContentPart[] = [
  {
    kind: 'text',
    text: [
      '## 摘要',
      '',
      '已经确认双二进制结构：`agent` 提供 HTTP API，`hub` 反代并托管客户端。',
      '',
      '- 已完成：SSE 回放与 resync、executor 的 link 帧',
      '- 未完成：浏览器端 e2e 的取消用例仍需修正',
    ].join('\n'),
    providerMeta: null,
  },
];

const skeleton: PartSkeleton[] = [{ seq: 0, kind: 'text', size: 220, state: 'complete' }];

export const Summary: Story = {
  args: { parts: summary, skeleton },
};

export const AwaitingBody: Story = {
  args: { parts: null, skeleton },
};
