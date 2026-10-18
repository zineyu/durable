import type { Meta, StoryObj } from '@storybook/react-vite';
import { CodeBlock } from './CodeBlock';
import { codeSample, inlineText } from '../fixtures/view';

const meta = {
  title: 'Components/CodeBlock',
  component: CodeBlock,
  parameters: { layout: 'fullscreen', frame: 'content' },
} satisfies Meta<typeof CodeBlock>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Rust: Story = {
  args: { code: codeSample, language: 'rust' },
};

export const NoLanguage: Story = {
  args: { code: inlineText },
};

export const LongLine: Story = {
  args: {
    language: 'bash',
    code: 'cargo run -p agent --bin export-types -- --output client/src/api/generated --check',
  },
};
