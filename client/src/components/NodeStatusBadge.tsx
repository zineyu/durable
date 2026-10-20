import type { NodeStatus } from '../api/generated/NodeStatus';
import { StatusDot } from './StatusDot';
import styles from './NodeStatusBadge.module.css';
import { nodeStatusLabel, nodeStatusTone } from '../view/labels';

export type NodeStatusBadgeProps = {
  status: NodeStatus;
  detail?: string | null;
};

export function NodeStatusBadge({ status, detail }: NodeStatusBadgeProps) {
  const label = nodeStatusLabel[status];
  return (
    <span
      className={styles.badge}
      data-status={status}
      title={detail === null || detail === undefined ? label : `${label}：${detail}`}
    >
      <StatusDot tone={nodeStatusTone[status]} />
      {label}
    </span>
  );
}
