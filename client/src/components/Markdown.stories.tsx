import type { Meta, StoryObj } from '@storybook/react-vite';
import { Markdown } from './Markdown';
import { inlineText } from '../fixtures/view';

const meta = {
  title: 'Components/Markdown',
  component: Markdown,
  parameters: { layout: 'fullscreen', frame: 'content' },
} satisfies Meta<typeof Markdown>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Paragraphs: Story = {
  args: {
    text: [
      '## 写文件的前置条件',
      '',
      '父目录必须已经存在。工具不会隐式创建目录。',
      '',
      '失败时错误作为工具结果回传模型，run 继续。',
    ].join('\n'),
  },
};

export const InlineCode: Story = {
  args: {
    text: `${inlineText}\n\n路径用行内代码标注：\`crates/agent/src/sse.rs\`。`,
  },
};

export const FencedCode: Story = {
  args: {
    text: ['重新生成协议类型：', '', '```bash', 'devenv shell -- durable-typegen', '```'].join(
      '\n',
    ),
  },
};

export const Lists: Story = {
  args: {
    text: [
      '排队规则：',
      '',
      '- session 内 FIFO',
      '- 全局并发槽位默认 4',
      '',
      '1. 入队即持久化',
      '2. run 结束后取下一项',
    ].join('\n'),
  },
};

export const LongParagraph: Story = {
  args: {
    text: [
      '取消一次运行中的 run 时，server 中止在途模型请求、杀掉 bash 进程组，把已收到的部分 assistant 文本作为已取消消息保留，队列继续执行下一项。',
      '',
      '重启 agent 时，仍为 running 的 run 被标记为 failed，原因为进程中断。',
    ].join('\n'),
  },
};
