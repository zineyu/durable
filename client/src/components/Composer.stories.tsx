import type { Meta, StoryObj } from '@storybook/react-vite';
import { Composer } from './Composer';

const meta = {
  title: 'Components/Composer',
  component: Composer,
  parameters: { layout: 'fullscreen' },
} satisfies Meta<typeof Composer>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Idle: Story = { args: {} };

export const Draft: Story = {
  args: { draft: '把 health 端点补一个回环地址测试。' },
};

export const Busy: Story = {
  args: { busy: true, draft: '队列中的下一条消息' },
};

export const Disabled: Story = {
  args: { disabled: true },
};
