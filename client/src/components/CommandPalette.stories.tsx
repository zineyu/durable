import type { Meta, StoryObj } from '@storybook/react-vite';
import { CommandPalette } from './CommandPalette';
import { toSessionId } from '../view/ids';
import type { PaletteEntry } from '../view/command-palette';

const entries: PaletteEntry[] = [
  {
    id: 'session:studio:s-3',
    group: 'session',
    label: '类型生成一致性检查',
    detail: 'studio · /Users/zine/space/project/durable',
    action: { kind: 'session', node: 'studio', sessionId: toSessionId('s-3') },
  },
  {
    id: 'session:macbook-air-01:s-1',
    group: 'session',
    label: '双二进制骨架与工程基线',
    detail: 'macbook-air-01 · /Users/zine/space/project/durable',
    action: { kind: 'session', node: 'macbook-air-01', sessionId: toSessionId('s-1') },
  },
  {
    id: 'action:register-node',
    group: 'action',
    label: '注册节点',
    detail: '把一台机器加入节点表',
    action: { kind: 'register-node' },
  },
  {
    id: 'action:cancel-run:macbook-air-01:s-1',
    group: 'action',
    label: '停止 run',
    detail: '双二进制骨架与工程基线 · macbook-air-01',
    action: { kind: 'cancel-run', node: 'macbook-air-01', sessionId: toSessionId('s-1') },
  },
  {
    id: 'node:studio',
    group: 'node',
    label: 'studio',
    detail: '可用',
    action: { kind: 'node', node: 'studio' },
  },
];

const meta = {
  title: 'Components/CommandPalette',
  component: CommandPalette,
  parameters: { layout: 'fullscreen', frame: 'page' },
} satisfies Meta<typeof CommandPalette>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Sessions: Story = {
  args: {
    entries,
    selectedIndex: 0,
    query: '',
    onQueryChange: () => undefined,
    onMove: () => undefined,
    onHover: () => undefined,
    onSelect: () => undefined,
    onClose: () => undefined,
  },
};

export const Filtered: Story = {
  args: {
    ...Sessions.args,
    entries: entries.slice(0, 2),
    query: '类型',
    selectedIndex: 1,
  },
};

export const NoMatch: Story = {
  args: {
    ...Sessions.args,
    entries: [],
    query: '不存在的条目',
  },
};
