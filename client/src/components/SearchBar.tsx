import type { KeyboardEvent, ReactElement } from 'react';
import { IconButton } from './IconButton';
import styles from './SearchBar.module.css';

export type SearchBarProps = {
  query: string;
  current: number;
  total: number;
  onQueryChange: (query: string) => void;
  onStep: (delta: number) => void;
  onClose: () => void;
};

export function SearchBar({
  query,
  current,
  total,
  onQueryChange,
  onStep,
  onClose,
}: SearchBarProps): ReactElement {
  const handleKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'Enter') {
      event.preventDefault();
      onStep(event.shiftKey ? -1 : 1);
      return;
    }
    if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      onClose();
    }
  };

  const status = total === 0 ? (query.trim().length === 0 ? '' : '无命中') : `${current + 1}/${total}`;

  return (
    <div className={styles.bar} role="search" aria-label="session 内搜索">
      <input
        className={styles.input}
        aria-label="搜索消息"
        placeholder="在 session 内搜索"
        autoFocus
        value={query}
        onChange={(event) => { onQueryChange(event.target.value); }}
        onKeyDown={handleKeyDown}
      />
      <span className={styles.status} aria-live="polite">
        {status}
      </span>
      <IconButton
        icon="up"
        label="上一个命中"
        onClick={() => { onStep(-1); }}
        disabled={total === 0}
      />
      <IconButton
        icon="down"
        label="下一个命中"
        onClick={() => { onStep(1); }}
        disabled={total === 0}
      />
      <IconButton icon="close" label="关闭搜索" onClick={onClose} />
    </div>
  );
}
