import type { ReactElement } from 'react';
import type { SubmitMode } from '../api/generated/SubmitMode';
import { IconButton } from './IconButton';
import styles from './Composer.module.css';

export type ComposerProps = {
  value: string;
  onChange: (value: string) => void;
  onSend?: () => void;
  onCancel?: () => void;
  disabled?: boolean;
  busy?: boolean;
  mode?: SubmitMode;
  onModeChange?: (mode: SubmitMode) => void;
};

const MODES: { mode: SubmitMode; label: string }[] = [
  { mode: 'steer', label: '插话' },
  { mode: 'queue', label: '排队' },
];

const PLACEHOLDERS: Record<SubmitMode, string> = {
  steer: '插话会在下一次模型调用前生效',
  queue: '当前 run 结束后执行队列中的消息',
};

export function Composer({
  value,
  onChange,
  onSend,
  onCancel,
  disabled = false,
  busy = false,
  mode = 'queue',
  onModeChange,
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
        placeholder={busy ? PLACEHOLDERS[mode] : '描述要完成的任务'}
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
        {busy ? (
          <div className={styles.modes} role="group" aria-label="发送语义">
            {MODES.map((entry) => (
              <button
                key={entry.mode}
                type="button"
                className={styles.mode}
                aria-pressed={mode === entry.mode}
                onClick={() => { onModeChange?.(entry.mode); }}
              >
                {entry.label}
              </button>
            ))}
          </div>
        ) : null}
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
