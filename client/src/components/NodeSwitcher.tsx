import type { NodeName } from '../api/generated/NodeName';
import { StatusDot } from './StatusDot';
import styles from './NodeSwitcher.module.css';
import { nodeStatusLabel, nodeStatusTone } from '../view/labels';
import type { NodeRecord } from '../view/types';

export type NodeSwitcherProps = {
  nodes: NodeRecord[];
  selected: NodeName | null;
  onSelect?: (node: NodeName) => void;
};

export function NodeSwitcher({ nodes, selected, onSelect }: NodeSwitcherProps) {
  return (
    <nav className={styles.switcher} aria-label="节点切换器">
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
            onClick={() => onSelect?.(record.name)}
          >
            <StatusDot tone={nodeStatusTone[record.status]} />
            <span className={styles.name}>{record.name}</span>
          </button>
        );
      })}
    </nav>
  );
}
