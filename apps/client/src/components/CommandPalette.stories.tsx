import type { Meta, StoryObj } from '@storybook/react-vite';
import { CommandPalette } from './CommandPalette';
import { toSessionId } from '../view/ids';
import type { PaletteEntry } from '../view/command-palette';

const entries: PaletteEntry[] = [
  {
    id: 'session:macbook-air-01:s-1',
    group: 'session',
    heading: 'macbook-air-01',
    label: '双二进制骨架与工程基线',
    detail: 'macbook-air-01 · /Users/zine/space/project/durable',
    action: { kind: 'session', sessionId: toSessionId('s-1') },
  },
  {
    id: 'session:studio:s-3',
    group: 'session',
    heading: 'studio',
    label: '类型生成一致性检查',
    detail: 'studio · /Users/zine/space/project/durable',
    action: { kind: 'session', sessionId: toSessionId('s-3') },
  },
  {
    id: 'action:register-host',
    group: 'action',
    heading: '动作',
    label: '注册 host',
    detail: '把一台机器加入 host 表',
    action: { kind: 'register-host' },
  },
  {
    id: 'action:cancel-run:s-1',
    group: 'action',
    heading: '动作',
    label: '停止 run',
    detail: '双二进制骨架与工程基线 · macbook-air-01',
    action: { kind: 'cancel-run', sessionId: toSessionId('s-1') },
  },
  {
    id: 'host:studio',
    group: 'host',
    heading: 'host',
    label: 'studio',
    detail: '可用 · 48 ms',
    action: { kind: 'host', host: 'studio' },
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
