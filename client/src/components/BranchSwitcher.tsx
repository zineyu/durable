import styles from './BranchSwitcher.module.css';

export type BranchSwitcherProps = {
  index: number;
  total: number;
  onPrev?: () => void;
  onNext?: () => void;
};

export function BranchSwitcher({ index, total, onPrev, onNext }: BranchSwitcherProps) {
  return (
    <span className={styles.switcher}>
      <button
        type="button"
        className={styles.step}
        onClick={onPrev}
        disabled={index <= 1 || onPrev === undefined}
        aria-label="上一个分支"
      >
        ‹
      </button>
      <span className={styles.counter}>
        {index}/{total}
      </span>
      <button
        type="button"
        className={styles.step}
        onClick={onNext}
        disabled={index >= total || onNext === undefined}
        aria-label="下一个分支"
      >
        ›
      </button>
    </span>
  );
}
