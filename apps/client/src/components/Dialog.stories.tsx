import type { Meta, StoryObj } from '@storybook/react-vite';
import { Dialog } from './Dialog';

const meta = {
  title: 'Components/Dialog',
  component: Dialog,
  parameters: { layout: 'fullscreen' },
} satisfies Meta<typeof Dialog>;

export default meta;

type Story = StoryObj<typeof meta>;

export const Confirm: Story = {
  args: {
    title: '确认重试',
    confirmLabel: '重试',
    onConfirm: () => undefined,
    onCancel: () => undefined,
    children: <p>重试以父用户消息为锚新建 run，产生同父的 assistant 兄弟节点。此操作不可撤销。</p>,
  },
};

export const Editing: Story = {
  args: {
    title: '编辑用户消息',
    confirmLabel: '确认编辑',
    onConfirm: () => undefined,
    onCancel: () => undefined,
    children: (
      <textarea
        defaultValue="把 ui 的静态资源改为内嵌，并确认 SSE 不被缓冲。"
        aria-label="消息正文"
        rows={4}
      />
    ),
  },
};

export const RegisterWorkspace: Story = {
  args: {
    title: '注册 workspace',
    confirmLabel: '注册',
    onConfirm: () => undefined,
    onCancel: () => undefined,
    children: <input defaultValue="/Users/zine/space/project/durable" aria-label="workspace 路径" />,
  },
};
