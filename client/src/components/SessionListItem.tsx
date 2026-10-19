import { useState } from 'react';
import type { FocusEvent, KeyboardEvent } from 'react';
import { IconButton } from './IconButton';
import { StatusDot } from './StatusDot';
import styles from './SessionListItem.module.css';
import { sessionStateTone } from '../view/labels';
import type { SessionSummary } from '../view/types';
import type { SessionId } from '../view/ids';

export type SessionItemView = 'default' | 'renaming' | 'deleting';

export type SessionListItemProps = {
  session: SessionSummary;
  selected: boolean;
  view?: SessionItemView;
  onSelect?: (id: SessionId) => void;
  onRenameRequest?: (id: SessionId) => void;
  onRenameSubmit?: (id: SessionId, title: string) => void;
  onEditCancel?: () => void;
  onDeleteRequest?: (id: SessionId) => void;
  onDeleteConfirm?: (id: SessionId) => void;
  defaultMenuOpen?: boolean;
};

export function SessionListItem({
  session,
  selected,
  view = 'default',
  onSelect,
  onRenameRequest,
  onRenameSubmit,
  onEditCancel,
  onDeleteRequest,
  onDeleteConfirm,
  defaultMenuOpen = false,
}: SessionListItemProps) {
  const [menuOpen, setMenuOpen] = useState(defaultMenuOpen);
  const menuVisible = menuOpen || view === 'deleting';

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

  const handleRenameKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'Enter') {
      const title = event.currentTarget.value.trim();
      if (title.length > 0) {
        onRenameSubmit?.(session.id, title);
      }
    }
    if (event.key === 'Escape') {
      onEditCancel?.();
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
      {view === 'renaming' ? (
        <div className={styles.rename}>
          <StatusDot tone={sessionStateTone[session.state]} className={styles.dot} />
          <input
            className={styles.input}
            defaultValue={session.title}
            aria-label="session 标题"
            autoFocus
            onKeyDown={handleRenameKeyDown}
            onBlur={() => onEditCancel?.()}
          />
        </div>
      ) : (
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
          {session.queuedCount > 0 ? (
            <span className={styles.badge}>{session.queuedCount}</span>
          ) : null}
        </button>
      )}
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
          aria-expanded={menuVisible}
          aria-haspopup="true"
          onClick={() => setMenuOpen(true)}
        />
        {menuVisible ? (
          <div className={styles.menuPanel}>
            {view === 'deleting' ? (
              <>
                <IconButton
                  icon="trash"
                  label="确认删除"
                  tone="danger"
                  size="md"
                  onClick={() => onDeleteConfirm?.(session.id)}
                />
                <IconButton
                  icon="close"
                  label="取消删除"
                  size="md"
                  onClick={() => onEditCancel?.()}
                />
              </>
            ) : (
              <>
                <IconButton
                  icon="pencil"
                  label="重命名"
                  size="md"
                  onClick={() => onRenameRequest?.(session.id)}
                />
                <IconButton
                  icon="trash"
                  label="删除"
                  tone="danger"
                  size="md"
                  onClick={() => onDeleteRequest?.(session.id)}
                />
              </>
            )}
          </div>
        ) : null}
      </div>
    </div>
  );
}
