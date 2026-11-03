import type { ReactElement } from 'react';
import { StatusDot } from './StatusDot';
import styles from './ConnectionChip.module.css';
import { connectionLabel, connectionTone } from '../view/labels';
import type { ConnectionState } from '../view/types';

export type ConnectionChipProps = {
  state: ConnectionState;
};

export function ConnectionChip({ state }: ConnectionChipProps): ReactElement {
  return (
    <span className={styles.chip}>
      <StatusDot tone={connectionTone[state]} />
      {connectionLabel[state]}
    </span>
  );
}
