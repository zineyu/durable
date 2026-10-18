import { toQueueItemId, toSessionId, toWorkspaceId, toWorkspacePath } from '../view/ids';
import type { QueueItem, WorkspaceSummary } from '../view/types';

export const modelName = 'gpt-5-codex';

export const workspaces: WorkspaceSummary[] = [
  {
    id: toWorkspaceId('ws-durable'),
    path: toWorkspacePath('/Users/zine/space/project/durable'),
    sessions: [
      {
        id: toSessionId('s-1'),
        title: '双二进制骨架与工程基线',
        state: 'running',
        queuedCount: 0,
      },
      {
        id: toSessionId('s-2'),
        title: 'UI 反代与静态资源内嵌',
        state: 'queued',
        queuedCount: 2,
      },
      {
        id: toSessionId('s-3'),
        title: '类型生成一致性检查',
        state: 'idle',
        queuedCount: 0,
      },
    ],
  },
  {
    id: toWorkspaceId('ws-agent'),
    path: toWorkspacePath('/Users/zine/space/project/agent-lab'),
    sessions: [
      {
        id: toSessionId('s-4'),
        title: 'SQLite 迁移与恢复',
        state: 'idle',
        queuedCount: 0,
      },
    ],
  },
];

export const workspaceWithoutSessions: WorkspaceSummary[] = [
  {
    id: toWorkspaceId('ws-empty'),
    path: toWorkspacePath('/Users/zine/space/project/durable'),
    sessions: [],
  },
];

export const queueItems: QueueItem[] = [
  { id: toQueueItemId('q-1'), text: '把 ui 的静态资源改为内嵌，并确认 SSE 不被缓冲' },
  { id: toQueueItemId('q-2'), text: '补一个 health 端点的回环地址测试' },
];

export const longQueueItem =
  '检查 ts-rs 在枚举与可选字段上的输出是否与手写类型一致，并在 CI 中重新生成后比较差异，确保生成的协议类型始终与 Rust 定义同步';

export const inlineText =
  'health 端点返回 status 与版本号。CI 重新生成类型后比较差异，确保前端拿到的协议定义与 Rust 一致。';

export const codeSample = `#[derive(Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    Ok,
}`;
