import type { ReactElement } from 'react';
import { IconButton } from './IconButton';
import styles from './Composer.module.css';

export type ComposerProps = {
  value: string;
  onChange: (value: string) => void;
  onSend?: () => void;
  onCancel?: () => void;
  disabled?: boolean;
  busy?: boolean;
};

export function Composer({
  value,
  onChange,
  onSend,
  onCancel,
  disabled = false,
  busy = false,
}: ComposerProps): ReactElement {
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
        aria-label="消息"
        value={value}
        placeholder={busy ? '当前 run 结束后执行队列中的消息' : '描述要完成的任务'}
        rows={3}
        disabled={disabled}
        onChange={(event) => { onChange(event.target.value); }}
        onKeyDown={(event) => {
          if (event.key === 'Enter' && !event.shiftKey) {
            event.preventDefault();
            onSend?.();
          }
        }}
      />
      <div className={styles.actions}>
        <span className={styles.hint}>Enter 发送 · Shift+Enter 换行</span>
        <span className={styles.buttons}>
          {onCancel !== undefined ? (
            <IconButton icon="stop" label="取消" tone="danger" size="lg" onClick={onCancel} />
          ) : null}
          <IconButton
            icon="send"
            label="发送"
            tone="primary"
            size="lg"
            type="submit"
            disabled={disabled || value.trim().length === 0}
          />
        </span>
      </div>
    </form>
  );
}
