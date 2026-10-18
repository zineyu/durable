import type { Meta, StoryObj } from '@storybook/react-vite';
import { Button } from './Button';

const meta = {
  title: 'Components/Button',
  component: Button,
  parameters: { layout: 'fullscreen', frame: 'centered' },
} satisfies Meta<typeof Button>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Primary: Story = {
  args: { variant: 'primary', children: '发送' },
};

export const Secondary: Story = {
  args: { variant: 'secondary', children: '注册 workspace' },
};

export const Ghost: Story = {
  args: { variant: 'ghost', children: '重命名' },
};

export const Danger: Story = {
  args: { variant: 'danger', children: '删除 session' },
};

export const Disabled: Story = {
  args: { variant: 'primary', children: '发送', disabled: true },
};
