import type { Meta, StoryObj } from '@storybook/react-vite';
import type { ContentPart } from '../api/generated/ContentPart';
import type { PartSkeleton } from '../api/generated/PartSkeleton';
import type { ToolResultState } from '../api/generated/ToolResultState';
import { AssistantContent } from './AssistantContent';

const meta = {
  title: 'Components/AssistantContent',
  component: AssistantContent,
  parameters: { layout: 'fullscreen', frame: 'content' },
} satisfies Meta<typeof AssistantContent>;

export default meta;

type Story = StoryObj<typeof meta>;

function body(value: string): ContentPart {
  return { kind: 'text', text: value, providerMeta: null };
}

function call(id: string, name: string, argumentsText: string): ContentPart {
  return { kind: 'tool_call', callId: id, name, arguments: argumentsText, providerMeta: null };
}

function succeeded(content: string): ToolResultState {
  return { status: 'finished', outcome: { status: 'succeeded', content } };
}

function skeleton(...kinds: PartSkeleton['kind'][]): PartSkeleton[] {
  return kinds.map((kind, index) => ({ seq: index, kind, size: 1, state: 'complete' }));
}

const readCall = call('call_1', 'read_file', '{"path":"devenv.nix"}');
const failedCall = call(
  'call_2',
  'edit_file',
  '{"path":"crates/core/src/store.rs","old_string":"let x","new_string":"let y"}',
);

export const Text: Story = {
  args: {
    parts: [
      body(
        '先确认 ui 的构建顺序，再验证反代路径。协议类型从 Rust 定义导出，前端只消费生成结果。',
      ),
    ],
    skeleton: skeleton('text'),
    toolResults: {},
  },
};

export const WithTools: Story = {
  args: {
    parts: [body('读取 devenv.nix 后确认构建顺序。'), readCall],
    skeleton: skeleton('text', 'tool_call'),
    toolResults: { call_1: succeeded('scripts.durable-build.exec = "durable-build"') },
  },
};

export const WithFailedTool: Story = {
  args: {
    parts: [body('编辑因为匹配不唯一而失败，run 继续。'), readCall, failedCall],
    skeleton: skeleton('text', 'tool_call', 'tool_call'),
    toolResults: {
      call_1: succeeded('scripts.durable-build.exec = "durable-build"'),
      call_2: {
        status: 'finished',
        outcome: { status: 'failed', code: 'edit_not_unique', message: '旧文本匹配到 3 处' },
      },
    },
  },
};

export const WithUnknownTool: Story = {
  args: {
    parts: [body('工具执行中进程崩溃，结局只能记为未知。'), readCall],
    skeleton: skeleton('text', 'tool_call'),
    toolResults: { call_1: { status: 'unknown' } },
  },
};

export const WithRunningTool: Story = {
  args: {
    parts: [body('开始执行工具。'), readCall],
    skeleton: skeleton('text', 'tool_call'),
    toolResults: {},
  },
};

export const WithReasoning: Story = {
  args: {
    parts: [
      {
        kind: 'reasoning',
        text: '先确认 workspace 边界，再决定读哪个文件。README 里说明了构建顺序。',
        providerMeta: null,
      },
      body('README 说明这是端到端测试的 workspace。'),
    ],
    skeleton: skeleton('reasoning', 'text'),
    toolResults: {},
  },
};

export const MarkdownText: Story = {
  args: {
    parts: [
      body(
        [
          '## 检查结果',
          '',
          '`ui` 反代不缓冲 SSE；`agent` 通过 `after` 游标续传。',
          '',
          '- 协议类型由 Rust 导出',
          '- 生成文件随协议变更提交',
          '',
          '1. 先取快照',
          '2. 再回放尾部',
          '',
          '> 断线后先刷新 bootstrap 与当前 session tree。',
          '',
          '**重点**：游标只能回退，不能跳过未应用的事件。',
        ].join('\n'),
      ),
    ],
    skeleton: skeleton('text'),
    toolResults: {},
  },
};

export const MarkdownCode: Story = {
  args: {
    parts: [
      body(
        [
          '重建事件流时使用快照水位作为游标：',
          '',
          '```ts',
          'const cursor = snapshotCursor(previous, tree.maxEventId);',
          'stream.resume(cursor);',
          '```',
          '',
          '```',
          'devenv shell -- durable-test',
          '```',
        ].join('\n'),
      ),
      readCall,
    ],
    skeleton: skeleton('text', 'tool_call'),
    toolResults: { call_1: succeeded('scripts.durable-build.exec = "durable-build"') },
  },
};

export const AwaitingBody: Story = {
  args: {
    parts: null,
    skeleton: skeleton('reasoning', 'text', 'tool_call'),
    toolResults: {},
  },
};
