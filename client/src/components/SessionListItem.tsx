import { useState } from 'react';
import type { FocusEvent, KeyboardEvent } from 'react';
import { IconButton } from './IconButton';
import { StatusDot } from './StatusDot';
import styles from './SessionListItem.module.css';
import { sessionStateTone } from '../view/labels';
import type { SessionSummary } from '../view/types';
import type { SessionId } from '../view/ids';

export type SessionListItemProps = {
  session: SessionSummary;
  selected: boolean;
  onSelect?: (id: SessionId) => void;
  onRename?: (id: SessionId) => void;
  onDelete?: (id: SessionId) => void;
  defaultMenuOpen?: boolean;
};

export function SessionListItem({
  session,
  selected,
  onSelect,
  onRename,
  onDelete,
  defaultMenuOpen = false,
}: SessionListItemProps) {
  const [menuOpen, setMenuOpen] = useState(defaultMenuOpen);

  const handleBlur = (event: FocusEvent<HTMLDivElement>) => {
    if (event.relatedTarget instanceof Node && event.currentTarget.contains(event.relatedTarget)) {
      return;
    }
    setMenuOpen(false);
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === 'Escape') {
      setMenuOpen(false);
    }
  };

  const menuLabel = `session 操作：${session.title}`;

  return (
    <div
      className={styles.item}
      data-selected={selected}
      onBlur={handleBlur}
      onKeyDown={handleKeyDown}
    >
      <button
        type="button"
        className={styles.select}
        aria-current={selected}
        onClick={() => {
          setMenuOpen(false);
          onSelect?.(session.id);
        }}
      >
        <StatusDot tone={sessionStateTone[session.state]} className={styles.dot} />
        <span className={styles.title}>
          <span className={styles.titleText}>{session.title}</span>
        </span>
        {session.queuedCount > 0 ? <span className={styles.badge}>{session.queuedCount}</span> : null}
      </button>
      <div
        className={styles.menu}
        onMouseEnter={() => setMenuOpen(true)}
        onMouseLeave={() => setMenuOpen(false)}
        onFocus={() => setMenuOpen(true)}
        onBlur={handleBlur}
        onKeyDown={handleKeyDown}
      >
        <IconButton
          icon="more"
          label={menuLabel}
          aria-expanded={menuOpen}
          aria-haspopup="true"
          onClick={() => setMenuOpen(true)}
        />
        {menuOpen ? (
          <div className={styles.menuPanel}>
            <IconButton
              icon="pencil"
              label="重命名"
              size="md"
              onClick={() => onRename?.(session.id)}
            />
            <IconButton
              icon="trash"
              label="删除"
              tone="danger"
              size="md"
              onClick={() => onDelete?.(session.id)}
            />
          </div>
        ) : null}
      </div>
    </div>
  );
}
