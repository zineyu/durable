import { StatusDot } from './StatusDot';
import styles from './ToolCard.module.css';
import { toolStateLabel, toolStateTone } from '../view/labels';
import type { ToolState } from '../view/types';

export type ToolCardProps = {
  name: string;
  state: ToolState;
  summary: string;
  input: string;
  result?: string;
  truncated?: boolean;
  durationLabel?: string;
  defaultOpen?: boolean;
};

export function ToolCard({
  name,
  state,
  summary,
  input,
  result,
  truncated = false,
  durationLabel,
  defaultOpen = false,
}: ToolCardProps) {
  return (
    <details className={styles.card} open={defaultOpen}>
      <summary className={styles.header}>
        <StatusDot tone={toolStateTone[state]} />
        <span className={styles.name}>{name}</span>
        <span className={styles.summary} title={summary}>
          {summary}
        </span>
        {durationLabel !== undefined ? (
          <span className={styles.duration}>{durationLabel}</span>
        ) : null}
        <span className={styles.state} data-tone={toolStateTone[state]}>
          {toolStateLabel[state]}
        </span>
      </summary>
      <div className={styles.body}>
        <section className={styles.section}>
          <h3 className={styles.sectionTitle}>入参</h3>
          <pre className={styles.code}>{input}</pre>
        </section>
        {result !== undefined ? (
          <section className={styles.section}>
            <h3 className={styles.sectionTitle}>结果</h3>
            <pre className={styles.code}>{result}</pre>
            {truncated ? <p className={styles.truncated}>输出已截断</p> : null}
          </section>
        ) : null}
      </div>
    </details>
  );
}
