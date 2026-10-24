import type { ReactElement } from 'react';
import { Button } from './Button';
import styles from './ShortcutHelp.module.css';
import { shortcutHelp } from '../view/shortcuts';

export type ShortcutHelpProps = {
  onClose: () => void;
};

export function ShortcutHelp({ onClose }: ShortcutHelpProps): ReactElement {
  return (
    <div className={styles.overlay} onClick={onClose}>
      <div
        className={styles.panel}
        role="dialog"
        aria-modal="true"
        aria-label="快捷键"
        onClick={(event) => { event.stopPropagation(); }}
      >
        <h2 className={styles.title}>快捷键</h2>
        <dl className={styles.list}>
          {shortcutHelp.map((entry) => (
            <div key={entry.keys} className={styles.row}>
              <dt className={styles.keys}>{entry.keys}</dt>
              <dd className={styles.description}>{entry.description}</dd>
            </div>
          ))}
        </dl>
        <div className={styles.actions}>
          <Button variant="secondary" onClick={onClose}>
            关闭
          </Button>
        </div>
      </div>
    </div>
  );
}
