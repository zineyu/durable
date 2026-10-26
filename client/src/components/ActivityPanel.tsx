import type { ReactElement } from 'react';
import { IconButton } from './IconButton';
import { StatusDot } from './StatusDot';
import styles from './ActivityPanel.module.css';
import type { RunningEntry } from '../view/types';
import type { SessionId } from '../view/ids';

export type ActivityPanelProps = {
  entries: RunningEntry[];
  onSelect?: (node: RunningEntry['node'], sessionId: SessionId) => void;
  onCancel?: (node: RunningEntry['node'], sessionId: SessionId) => void;
};

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
        <ul className={styles.list}>
          {entries.map((entry) => (
            <li key={`${entry.node}:${entry.sessionId}`} className={styles.item}>
              <button
                type="button"
                className={styles.select}
                onClick={() => { onSelect?.(entry.node, entry.sessionId); }}
              >
                <StatusDot tone="running" />
                <span className={styles.text}>
                  <span className={styles.name}>{entry.title}</span>
                  <span className={styles.meta}>
                    {entry.node}
                    {entry.queueLength > 0 ? ` · 队列 ${entry.queueLength}` : ''}
                  </span>
                </span>
              </button>
              <IconButton
                icon="stop"
                label={`停止运行：${entry.title}`}
                tone="danger"
                onClick={() => { onCancel?.(entry.node, entry.sessionId); }}
              />
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
