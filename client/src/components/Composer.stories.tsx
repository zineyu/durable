import type { Meta, StoryObj } from '@storybook/react-vite';
import { Composer } from './Composer';

const meta = {
  title: 'Components/Composer',
  component: Composer,
  parameters: { layout: 'fullscreen' },
} satisfies Meta<typeof Composer>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Idle: Story = { args: { value: '', onChange: () => undefined } };

export const Draft: Story = {
  args: { value: '把 health 端点补一个回环地址测试。', onChange: () => undefined },
};

export const Busy: Story = {
  args: {
    value: '队列中的下一条消息',
    onChange: () => undefined,
    busy: true,
    onCancel: () => undefined,
  },
};

export const BusySteer: Story = {
  args: {
    value: '先别改迁移，只补领域类型',
    onChange: () => undefined,
    busy: true,
    mode: 'steer',
    onModeChange: () => undefined,
    onCancel: () => undefined,
  },
};

export const Disabled: Story = {
  args: { value: '', onChange: () => undefined, disabled: true },
};
