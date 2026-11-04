import type { ReactElement } from 'react';
import type { HostStatus } from '../api/generated/HostStatus';
import { StatusDot } from './StatusDot';
import styles from './HostStatusBadge.module.css';
import { hostStatusLabel, hostStatusTone } from '../view/labels';

export type HostStatusBadgeProps = {
  status: HostStatus;
  detail?: string | null;
};

export function HostStatusBadge({ status, detail }: HostStatusBadgeProps): ReactElement {
  const label = hostStatusLabel[status];
  return (
    <span
      className={styles.badge}
      data-status={status}
      title={detail === null || detail === undefined ? label : `${label}：${detail}`}
    >
      <StatusDot tone={hostStatusTone[status]} />
      {label}
    </span>
  );
}
