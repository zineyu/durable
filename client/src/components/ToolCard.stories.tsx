import type { Meta, StoryObj } from '@storybook/react-vite';
import { ToolCard } from './ToolCard';

const meta = {
  title: 'Components/ToolCard',
  component: ToolCard,
  parameters: { layout: 'fullscreen', frame: 'content' },
} satisfies Meta<typeof ToolCard>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Running: Story = {
  args: {
    name: 'read_file',
    state: 'running',
    summary: 'client/src/App.tsx',
    input: '{"path": "client/src/App.tsx"}',
    defaultOpen: true,
  },
};

export const Succeeded: Story = {
  args: {
    name: 'read_file',
    state: 'succeeded',
    summary: 'client/src/App.tsx',
    input: '{"path": "client/src/App.tsx", "start_line": 1, "end_line": 12}',
    result: '     1\timport { AppShell } from \'./components/AppShell\';\n     2\timport { EmptyState } from \'./components/EmptyState\';',
    durationLabel: '12ms',
    defaultOpen: true,
  },
};

export const Failed: Story = {
  args: {
    name: 'edit_file',
    state: 'failed',
    summary: 'client/src/App.tsx',
    input: '{"path": "client/src/App.tsx", "old_string": "EmptyState", "new_string": "Shell"}',
    result: 'edit.multiple_matches: old_string 在文件中出现 3 次，要求唯一匹配。',
    durationLabel: '4ms',
    defaultOpen: true,
  },
};

export const Unknown: Story = {
  args: {
    name: 'bash',
    state: 'unknown',
    summary: 'cargo test --workspace',
    input: '{"command": "cargo test --workspace"}',
    result: '工具结果未知：执行中崩溃，结局没有观测到。',
    defaultOpen: true,
  },
};

export const Truncated: Story = {
  args: {
    name: 'bash',
    state: 'succeeded',
    summary: 'cargo test --workspace',
    input: '{"command": "cargo test --workspace"}',
    result: 'running 6 tests\n......\n\ntest result: ok. 6 passed; 0 failed',
    truncated: true,
    durationLabel: '4.2s',
    defaultOpen: true,
  },
};

export const Collapsed: Story = {
  args: {
    name: 'bash',
    state: 'succeeded',
    summary: 'npm run build',
    input: '{"command": "npm run build"}',
    result: 'dist/assets/index-4f2a.js  142.30 kB',
    durationLabel: '1.8s',
  },
};

export const BashNonZeroExit: Story = {
  args: {
    name: 'bash',
    state: 'succeeded',
    summary: 'cargo fmt --all --check',
    input: '{"command": "cargo fmt --all --check"}',
    result:
      '退出码: 3\n耗时: 412 ms\n输出:\nDiff in /crates/agent/src/sse.rs:147:\n-            reason: ResyncReason::Lagged { skipped: 3 }',
    durationLabel: '412ms',
    defaultOpen: true,
  },
};

export const BashTimeout: Story = {
  args: {
    name: 'bash',
    state: 'failed',
    summary: 'cargo test --workspace -- --nocapture',
    input: '{"command": "cargo test --workspace -- --nocapture"}',
    result: 'bash_timeout: 命令超过 120000 ms 未结束',
    durationLabel: '120s',
    defaultOpen: true,
  },
};
