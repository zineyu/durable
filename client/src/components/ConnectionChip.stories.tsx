import type { Meta, StoryObj } from '@storybook/react-vite';
import { ConnectionChip } from './ConnectionChip';

const meta = {
  title: 'Components/ConnectionChip',
  component: ConnectionChip,
  parameters: { layout: 'fullscreen', frame: 'centered' },
} satisfies Meta<typeof ConnectionChip>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Connected: Story = { args: { state: 'connected' } };

export const Reconnecting: Story = { args: { state: 'reconnecting' } };

export const Disconnected: Story = { args: { state: 'disconnected' } };
