import styles from './StatusDot.module.css';
import type { StatusTone } from '../view/types';

export type StatusDotProps = {
  tone: StatusTone;
  label?: string;
  className?: string | undefined;
};

export function StatusDot({ tone, label, className }: StatusDotProps) {
  const classes = [styles.dot, className].filter(Boolean).join(' ');
  return (
    <span
      className={classes}
      data-tone={tone}
      role={label ? 'img' : undefined}
      aria-label={label}
    />
  );
}
