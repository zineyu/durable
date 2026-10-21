import type { ReactElement } from 'react';
import { IconButton } from './IconButton';
import styles from './QueueStrip.module.css';
import type { QueueItem } from '../view/types';

export type QueueStripProps = {
  items: QueueItem[];
  onRemove?: (id: QueueItem['id']) => void;
};

export function QueueStrip({ items, onRemove }: QueueStripProps): ReactElement {
  return (
    <section className={styles.strip}>
      <h2 className={styles.title}>队列</h2>
      {items.length === 0 ? (
        <p className={styles.empty}>暂无排队消息</p>
      ) : (
        <ul className={styles.list}>
          {items.map((item) => (
            <li key={item.id} className={styles.item}>
              <span className={styles.text} title={item.text}>
                {item.text}
              </span>
              <IconButton
                icon="close"
                label="移除队列项"
                tone="danger"
                onClick={() => onRemove?.(item.id)}
              />
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
