import type { ReactElement } from 'react';
import { StatusDot } from './StatusDot';
import styles from './ConnectionChip.module.css';
import { connectionTone } from '../view/labels';
import type { ConnectionState } from '../view/types';

export type ConnectionChipProps = {
  state: ConnectionState;
};

const labels: Record<ConnectionState, string> = {
  connected: '已连接',
  reconnecting: '重连中',
  disconnected: '已断开',
};

export function ConnectionChip({ state }: ConnectionChipProps): ReactElement {
  return (
    <span className={styles.chip}>
      <StatusDot tone={connectionTone[state]} />
      {labels[state]}
    </span>
  );
}
