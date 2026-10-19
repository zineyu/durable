import type { Meta, StoryObj } from '@storybook/react-vite';
import { AssistantContent } from './AssistantContent';
import type { ToolCallRecord } from '../api/generated/ToolCallRecord';

const meta = {
  title: 'Components/AssistantContent',
  component: AssistantContent,
  parameters: { layout: 'fullscreen', frame: 'content' },
} satisfies Meta<typeof AssistantContent>;

export default meta;

type Story = StoryObj<typeof meta>;

const readCall: ToolCallRecord = {
  id: 'call_1',
  name: 'read_file',
  arguments: '{"path":"devenv.nix"}',
  outcome: { status: 'succeeded', content: 'scripts.durable-build.exec = "durable-build"' },
};

const failedCall: ToolCallRecord = {
  id: 'call_2',
  name: 'edit_file',
  arguments: '{"path":"crates/core/src/store.rs","old_string":"let x","new_string":"let y"}',
  outcome: { status: 'failed', code: 'edit_not_unique', message: '旧文本匹配到 3 处' },
};

export const Text: Story = {
  args: {
    text: '先确认 ui 的构建顺序，再验证反代路径。协议类型从 Rust 定义导出，前端只消费生成结果。',
    toolCalls: [],
  },
};

export const WithTools: Story = {
  args: {
    text: '读取 devenv.nix 后确认构建顺序。',
    toolCalls: [readCall],
  },
};

export const WithFailedTool: Story = {
  args: {
    text: '编辑因为匹配不唯一而失败，run 继续。',
    toolCalls: [readCall, failedCall],
  },
};
