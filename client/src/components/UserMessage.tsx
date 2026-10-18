import { IconButton } from './IconButton';
import styles from './UserMessage.module.css';

export type UserMessageProps = {
  text: string;
  onEdit?: () => void;
};

export function UserMessage({ text, onEdit }: UserMessageProps) {
  return (
    <article className={styles.message}>
      <p className={styles.bubble}>{text}</p>
      <div className={styles.actions}>
        <IconButton icon="pencil" label="编辑消息" onClick={onEdit} />
      </div>
    </article>
  );
}
