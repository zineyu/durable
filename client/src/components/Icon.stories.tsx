import type { Meta, StoryObj } from '@storybook/react-vite';
import { Icon } from './Icon';

const meta = {
  title: 'Components/Icon',
  component: Icon,
  parameters: { layout: 'fullscreen', frame: 'centered' },
} satisfies Meta<typeof Icon>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Pencil: Story = { args: { name: 'pencil' } };

export const Trash: Story = { args: { name: 'trash' } };

export const Close: Story = { args: { name: 'close' } };

export const More: Story = { args: { name: 'more' } };

export const Send: Story = { args: { name: 'send' } };
