import { Fragment, useEffect, useRef } from 'react';
import type { KeyboardEvent, ReactElement } from 'react';
import { Icon } from './Icon';
import styles from './CommandPalette.module.css';
import type { PaletteEntry } from '../view/command-palette';

export type CommandPaletteProps = {
  entries: PaletteEntry[];
  selectedIndex: number;
  query: string;
  onQueryChange: (query: string) => void;
  onMove: (delta: number) => void;
  onHover: (index: number) => void;
  onSelect: (entry: PaletteEntry) => void;
  onClose: () => void;
};

export function CommandPalette({
  entries,
  selectedIndex,
  query,
  onQueryChange,
  onMove,
  onHover,
  onSelect,
  onClose,
}: CommandPaletteProps): ReactElement {
  const listRef = useRef<HTMLDivElement | null>(null);
  const selected = entries[selectedIndex];

  useEffect(() => {
    const element = listRef.current?.querySelector('[data-selected="true"]');
    element?.scrollIntoView({ block: 'nearest' });
  }, [selectedIndex, entries.length, query]);

  const handleKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'ArrowDown') {
      event.preventDefault();
      onMove(1);
      return;
    }
    if (event.key === 'ArrowUp') {
      event.preventDefault();
      onMove(-1);
      return;
    }
    if (event.key === 'Enter') {
      event.preventDefault();
      if (selected !== undefined) {
        onSelect(selected);
      }
      return;
    }
    if (event.key === 'Escape') {
      event.preventDefault();
      onClose();
    }
  };

  return (
    <div className={styles.overlay} onClick={onClose}>
      <div
        className={styles.panel}
        role="dialog"
        aria-modal="true"
        aria-label="命令面板"
        onClick={(event) => { event.stopPropagation(); }}
      >
        <div className={styles.search}>
          <Icon name="search" />
          <input
            className={styles.input}
            aria-label="搜索命令"
            placeholder="搜索 session、动作或 host"
            role="combobox"
            aria-expanded="true"
            aria-controls="command-palette-list"
            aria-activedescendant={selected?.id}
            autoFocus
            value={query}
            onChange={(event) => { onQueryChange(event.target.value); }}
            onKeyDown={handleKeyDown}
          />
          <kbd className={styles.kbd}>Esc</kbd>
        </div>
        <div
          className={styles.list}
          id="command-palette-list"
          role="listbox"
          aria-label="命令"
          ref={listRef}
        >
          {entries.length === 0 ? (
            <p className={styles.empty}>没有匹配的条目</p>
          ) : (
            entries.map((entry, index) => (
              <Fragment key={entry.id}>
                {index === 0 || entries[index - 1]?.heading !== entry.heading ? (
                  <p className={styles.group}>{entry.heading}</p>
                ) : null}
                <button
                  type="button"
                  id={entry.id}
                  className={styles.item}
                  role="option"
                  aria-selected={index === selectedIndex}
                  data-selected={index === selectedIndex}
                  onMouseEnter={() => { onHover(index); }}
                  onClick={() => { onSelect(entry); }}
                >
                  <span className={styles.label}>{entry.label}</span>
                  <span className={styles.detail}>{entry.detail}</span>
                </button>
              </Fragment>
            ))
          )}
        </div>
      </div>
    </div>
  );
}
