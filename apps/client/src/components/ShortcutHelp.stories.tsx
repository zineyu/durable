import type { Meta, StoryObj } from '@storybook/react-vite';
import { ShortcutHelp } from './ShortcutHelp';

const meta = {
  title: 'Components/ShortcutHelp',
  component: ShortcutHelp,
  parameters: { layout: 'fullscreen', frame: 'page' },
} satisfies Meta<typeof ShortcutHelp>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Default: Story = {
  args: { onClose: () => undefined },
};
