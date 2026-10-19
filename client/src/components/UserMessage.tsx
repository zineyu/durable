import { BranchSwitcher } from './BranchSwitcher';
import { IconButton } from './IconButton';
import styles from './UserMessage.module.css';
import type { BranchControl } from '../view/types';

export type UserMessageProps = {
  text: string;
  onEdit?: () => void;
  editDisabled?: boolean;
  branch?: BranchControl;
};

export function UserMessage({ text, onEdit, editDisabled = false, branch }: UserMessageProps) {
  return (
    <article className={styles.message}>
      <p className={styles.bubble}>{text}</p>
      <div className={styles.actions}>
        <IconButton icon="pencil" label="编辑消息" onClick={onEdit} disabled={editDisabled} />
        {branch !== undefined ? (
          <BranchSwitcher
            index={branch.index}
            total={branch.total}
            {...(branch.onPrev ? { onPrev: branch.onPrev } : {})}
            {...(branch.onNext ? { onNext: branch.onNext } : {})}
          />
        ) : null}
      </div>
    </article>
  );
}
