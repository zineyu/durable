import type { Meta, StoryObj } from '@storybook/react-vite';
import { AssistantMessage } from './AssistantMessage';
import { CodeBlock } from './CodeBlock';
import { ToolCard } from './ToolCard';
import { Transcript } from './Transcript';
import { UserMessage } from './UserMessage';
import { codeSample } from '../fixtures/view';

const meta = {
  title: 'Components/Transcript',
  component: Transcript,
  parameters: { layout: 'fullscreen', frame: 'content' },
} satisfies Meta<typeof Transcript>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Conversation: Story = {
  args: {
    children: (
      <>
        <UserMessage text="把 ui 的静态资源改为内嵌，并确认 SSE 不被缓冲。" />
        <AssistantMessage state="complete">
          <p>先确认构建顺序，再验证反代路径。</p>
          <ToolCard
            name="read_file"
            state="succeeded"
            summary="devenv.nix"
            input='{"path": "devenv.nix"}'
            result="scripts.durable-build.exec"
            durationLabel="8ms"
          />
          <CodeBlock code={codeSample} language="rust" />
        </AssistantMessage>
        <UserMessage text="再补一条回环地址的测试。" />
        <AssistantMessage state="streaming">
          <p>回环地址测试已加入，接下来检查生成的协议类型</p>
        </AssistantMessage>
      </>
    ),
  },
};

export const LongConversation: Story = {
  args: {
    children: (
      <>
        {Array.from({ length: 6 }, (_, index) => (
          <UserMessage key={`user-${index}`} text={`第 ${index + 1} 条消息：检查 workspace 边界。`} />
        ))}
        <AssistantMessage state="complete">
          <p>六轮检查完成，边界条件均已覆盖。</p>
        </AssistantMessage>
      </>
    ),
  },
};
