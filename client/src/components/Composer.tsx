import { IconButton } from './IconButton';
import styles from './Composer.module.css';

export type ComposerProps = {
  draft?: string;
  disabled?: boolean;
  busy?: boolean;
  onSend?: () => void;
};

export function Composer({ draft = '', disabled = false, busy = false, onSend }: ComposerProps) {
  return (
    <form
      className={styles.composer}
      onSubmit={(event) => {
        event.preventDefault();
        onSend?.();
      }}
    >
      <textarea
        className={styles.input}
        defaultValue={draft}
        placeholder={busy ? '当前 run 结束后执行队列中的消息' : '描述要完成的任务'}
        rows={3}
        disabled={disabled}
      />
      <div className={styles.actions}>
        <span className={styles.hint}>Enter 发送 · Shift+Enter 换行</span>
        <IconButton
          icon="send"
          label="发送"
          tone="primary"
          size="lg"
          type="submit"
          disabled={disabled || busy}
        />
      </div>
    </form>
  );
}
