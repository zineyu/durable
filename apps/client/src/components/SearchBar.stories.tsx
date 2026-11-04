import type { Meta, StoryObj } from '@storybook/react-vite';
import { SearchBar } from './SearchBar';

const meta = {
  title: 'Components/SearchBar',
  component: SearchBar,
  parameters: { layout: 'fullscreen', frame: 'content' },
} satisfies Meta<typeof SearchBar>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Searching: Story = {
  args: {
    query: '迁移',
    current: 1,
    total: 4,
    onQueryChange: () => undefined,
    onStep: () => undefined,
    onClose: () => undefined,
  },
};

export const NoMatch: Story = {
  args: {
    ...Searching.args,
    query: '不存在的词',
    current: -1,
    total: 0,
  },
};
