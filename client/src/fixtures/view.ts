import { toQueueItemId, toSessionId, toWorkspaceId, toWorkspacePath } from '../view/ids';
import type { HostRecord, QueueItem, WorkspaceSummary } from '../view/types';

export const modelName = 'gpt-5-codex';

export const hosts: HostRecord[] = [
  {
    name: 'macbook-air-01',
    kind: 'local',
    target: null,
    enabled: true,
    status: 'ok',
    detail: null,
    latencyMs: 4,
  },
  {
    name: 'macbook-air-02',
    kind: 'ssh',
    target: 'macbook-air-02',
    enabled: true,
    status: 'ok',
    detail: null,
    latencyMs: 32,
  },
  {
    name: 'studio',
    kind: 'ssh',
    target: 'studio',
    enabled: true,
    status: 'unreachable',
    detail: '连接 host 失败: 隧道已断开',
    latencyMs: null,
  },
];

export const unreachableHost: HostRecord = {
  name: 'studio',
  kind: 'ssh',
  target: 'studio',
  enabled: true,
  status: 'unreachable',
  detail: '连接 host 失败: 隧道已断开',
  latencyMs: null,
};

export const incompatibleHost: HostRecord = {
  name: 'legacy-lab',
  kind: 'ssh',
  target: 'legacy-lab',
  enabled: true,
  status: 'incompatible',
  detail: 'executor 构建 0 与 agent 要求的 1 不一致',
  latencyMs: null,
};

const updated = (hours: number, minutes: number): number =>
  new Date(2026, 9, 19, hours, minutes).getTime();

export const workspaces: WorkspaceSummary[] = [
  {
    id: toWorkspaceId('macbook-air-01', '/Users/zine/space/project/durable'),
    host: 'macbook-air-01',
    path: toWorkspacePath('/Users/zine/space/project/durable'),
    sessions: [
      {
        id: toSessionId('s-1'),
        host: 'macbook-air-01',
        title: '双二进制骨架与工程基线',
        state: 'running',
        unread: false,
        outcome: null,
        queuedCount: 0,
        updatedAt: updated(15, 58),
      },
      {
        id: toSessionId('s-2'),
        host: 'macbook-air-01',
        title: 'UI 反代与静态资源内嵌',
        state: 'queued',
        unread: false,
        outcome: null,
        queuedCount: 2,
        updatedAt: updated(14, 12),
      },
      {
        id: toSessionId('s-3'),
        host: 'macbook-air-01',
        title: '类型生成一致性检查',
        state: 'idle',
        unread: false,
        outcome: null,
        queuedCount: 0,
        updatedAt: updated(9, 30),
      },
    ],
  },
  {
    id: toWorkspaceId('macbook-air-02', '/Users/zine/space/project/agent-lab'),
    host: 'macbook-air-02',
    path: toWorkspacePath('/Users/zine/space/project/agent-lab'),
    sessions: [
      {
        id: toSessionId('s-4'),
        host: 'macbook-air-02',
        title: 'SQLite 迁移与恢复',
        state: 'idle',
        unread: false,
        outcome: null,
        queuedCount: 0,
        updatedAt: updated(8, 5),
      },
    ],
  },
];

export const workspaceWithoutSessions: WorkspaceSummary[] = [
  {
    id: toWorkspaceId('local', '/Users/zine/space/empty'),
    host: 'local',
    path: toWorkspacePath('/Users/zine/space/project/durable'),
    sessions: [],
  },
];

export const queueItems: QueueItem[] = [
  { id: toQueueItemId('q-1'), text: '把 ui 的静态资源改为内嵌，并确认 SSE 不被缓冲', pendingSteer: false },
  { id: toQueueItemId('q-2'), text: '补一个 health 端点的回环地址测试', pendingSteer: false },
];

export const steerQueueItem: QueueItem = {
  id: toQueueItemId('q-steer'),
  text: '先不要改迁移，只把领域类型补上',
  pendingSteer: true,
};

export const longQueueItem =
  '检查 ts-rs 在枚举与可选字段上的输出是否与手写类型一致，并在 CI 中重新生成后比较差异，确保生成的协议类型始终与 Rust 定义同步';

export const inlineText =
  'health 端点返回 status 与版本号。CI 重新生成类型后比较差异，确保前端拿到的协议定义与 Rust 一致。';

export const codeSample = `#[derive(Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    Ok,
}`;
