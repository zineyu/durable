import type { HostName } from '../api/generated/HostName';
import type {
  HostView,
  PaletteSession,
  RunningEntry,
  SessionSummary,
  WorkspaceSummary,
} from './types';

/// 本模块负责给已派生的列表确定呈现顺序：侧栏顺序、相邻同 host 分组与命令面板的 host 顺序。
/// 这些函数只读参数，不读客户端状态；状态的唯一常规读取者是 `derived.ts`。

/// 侧栏展示顺序下的全部 session，供快捷键在同一顺序里切换。
export function orderedSessions(workspaces: WorkspaceSummary[]): SessionSummary[] {
  return workspaces.flatMap((workspace) => workspace.sessions);
}

/// 按 host 分组的条目。组的先后与组内顺序由分组函数决定。
export type HostGroup<T> = {
  host: HostName;
  entries: T[];
};

/// 相邻同 host 的条目合成一组，输入顺序不变。正在运行面板据此把同一台机器上的 run 并在一起。
export function hostGroups(entries: RunningEntry[]): HostGroup<RunningEntry>[] {
  const groups: HostGroup<RunningEntry>[] = [];
  for (const entry of entries) {
    const group = groups.at(-1);
    if (group?.host === entry.host) {
      group.entries.push(entry);
    } else {
      groups.push({ host: entry.host, entries: [entry] });
    }
  }
  return groups;
}

/// 命令面板的 session 顺序：host 顺序沿用 host 表，组内最近更新的在前。
export function hostOrderedSessions(
  sessions: PaletteSession[],
  hosts: HostView[],
): PaletteSession[] {
  const order = new Map(hosts.map((host, index) => [host.name, index]));
  return [...sessions].sort(
    (left, right) =>
      (order.get(left.host) ?? Number.MAX_SAFE_INTEGER) -
        (order.get(right.host) ?? Number.MAX_SAFE_INTEGER) ||
      right.updatedAt - left.updatedAt,
  );
}
