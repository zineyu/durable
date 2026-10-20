import { useState } from 'react';
import type { FocusEvent, KeyboardEvent } from 'react';
import type { NodeName } from '../api/generated/NodeName';
import { IconButton } from './IconButton';
import styles from './NodeSwitcher.module.css';
import { StatusDot } from './StatusDot';
import { nodeStatusLabel, nodeStatusTone } from '../view/labels';
import type { NodeRecord } from '../view/types';

export type NodeSwitcherProps = {
  nodes: NodeRecord[];
  selected: NodeName | null;
  onSelect?: (node: NodeName) => void;
  onRegister?: () => void;
  onRename?: (node: NodeName) => void;
  onRemove?: (node: NodeName) => void;
  defaultMenuOpen?: boolean;
};

export function NodeSwitcher({
  nodes,
  selected,
  onSelect,
  onRegister,
  onRename,
  onRemove,
  defaultMenuOpen = false,
}: NodeSwitcherProps) {
  const [menuOpen, setMenuOpen] = useState(defaultMenuOpen);

  const handleBlur = (event: FocusEvent<HTMLElement>) => {
    if (event.relatedTarget instanceof Node && event.currentTarget.contains(event.relatedTarget)) {
      return;
    }
    setMenuOpen(false);
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    if (event.key === 'Escape') {
      setMenuOpen(false);
    }
  };

  return (
    <nav
      className={styles.switcher}
      aria-label="节点切换器"
      onBlur={handleBlur}
      onKeyDown={handleKeyDown}
    >
      {nodes.length === 0 ? <p className={styles.empty}>还没有节点</p> : null}
      {nodes.map((record) => {
        const label = nodeStatusLabel[record.status];
        return (
          <button
            key={record.name}
            type="button"
            className={styles.node}
            aria-pressed={record.name === selected}
            title={
              record.detail === null
                ? `节点 ${record.name}：${label}`
                : `节点 ${record.name}：${label}。${record.detail}`
            }
            onClick={() => {
              setMenuOpen(false);
              onSelect?.(record.name);
            }}
          >
            <StatusDot tone={nodeStatusTone[record.status]} />
            <span className={styles.name}>{record.name}</span>
          </button>
        );
      })}
      <div className={styles.actions}>
        <IconButton icon="plus" label="注册节点" onClick={() => onRegister?.()} />
        {selected === null ? null : (
          <div className={styles.menu}>
            <IconButton
              icon="more"
              label={`节点操作：${selected}`}
              aria-haspopup="true"
              aria-expanded={menuOpen}
              onClick={() => setMenuOpen(true)}
            />
            {menuOpen ? (
              <div className={styles.menuPanel}>
                <IconButton
                  icon="pencil"
                  label="重命名节点"
                  size="md"
                  onClick={() => {
                    setMenuOpen(false);
                    onRename?.(selected);
                  }}
                />
                <IconButton
                  icon="trash"
                  label="删除节点"
                  tone="danger"
                  size="md"
                  onClick={() => {
                    setMenuOpen(false);
                    onRemove?.(selected);
                  }}
                />
              </div>
            ) : null}
          </div>
        )}
      </div>
    </nav>
  );
}
