import type { ReactElement } from 'react';
import { IconButton } from './IconButton';
import { StatusDot } from './StatusDot';
import styles from './ActivityPanel.module.css';
import type { RunningEntry } from '../view/types';
import type { SessionId } from '../view/ids';

export type ActivityPanelProps = {
  entries: RunningEntry[];
  onSelect?: (sessionId: SessionId) => void;
  onCancel?: (sessionId: SessionId) => void;
};

/// 正在运行面板。条目按 host 分组，同一台机器上的 run 挨在一起。
export function ActivityPanel({ entries, onSelect, onCancel }: ActivityPanelProps): ReactElement {
  return (
    <section className={styles.panel} aria-label="正在运行">
      <h2 className={styles.title}>
        <span>正在运行</span>
        <span className={styles.count}>{entries.length}</span>
      </h2>
      {entries.length === 0 ? (
        <p className={styles.empty}>没有正在运行的 run</p>
      ) : (
        groups(entries).map((group) => (
          <div key={group.host} className={styles.group}>
            <p className={styles.host}>{group.host}</p>
            <ul className={styles.list}>
              {group.entries.map((entry) => (
                <li key={`${entry.host}:${entry.sessionId}`} className={styles.item}>
                  <button
                    type="button"
                    className={styles.select}
                    onClick={() => { onSelect?.(entry.sessionId); }}
                  >
                    <StatusDot tone="running" />
                    <span className={styles.text}>
                      <span className={styles.name}>{entry.title}</span>
                      {entry.queueLength > 0 ? (
                        <span className={styles.meta}>队列 {entry.queueLength}</span>
                      ) : null}
                    </span>
                  </button>
                  <IconButton
                    icon="stop"
                    label={`停止运行：${entry.title}`}
                    tone="danger"
                    onClick={() => { onCancel?.(entry.sessionId); }}
                  />
                </li>
              ))}
            </ul>
          </div>
        ))
      )}
    </section>
  );
}

function groups(entries: RunningEntry[]): { host: string; entries: RunningEntry[] }[] {
  const grouped: { host: string; entries: RunningEntry[] }[] = [];
  for (const entry of entries) {
    const group = grouped.at(-1);
    if (group?.host === entry.host) {
      group.entries.push(entry);
    } else {
      grouped.push({ host: entry.host, entries: [entry] });
    }
  }
  return grouped;
}
