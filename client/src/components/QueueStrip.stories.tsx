import type { Meta, StoryObj } from '@storybook/react-vite';
import { QueueStrip } from './QueueStrip';
import { toQueueItemId } from '../view/ids';
import { longQueueItem, queueItems, steerQueueItem } from '../fixtures/view';

const meta = {
  title: 'Components/QueueStrip',
  component: QueueStrip,
  parameters: { layout: 'fullscreen' },
} satisfies Meta<typeof QueueStrip>;

export default meta;

type Story = StoryObj<typeof meta>;

export const TwoItems: Story = { args: { items: queueItems } };

export const WithPendingSteer: Story = {
  args: { items: [...queueItems, steerQueueItem] },
};

export const SingleLongItem: Story = {
  args: { items: [{ id: toQueueItemId('q-long'), text: longQueueItem, pendingSteer: false }] },
};

export const Empty: Story = { args: { items: [] } };
