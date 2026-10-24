import { useRef, useState } from 'react';
import type { ClipboardEvent, KeyboardEvent, ReactElement } from 'react';
import type { SubmitMode } from '../api/generated/SubmitMode';
import { expandPastes, foldPaste, stepHistory } from '../view/composer-input';
import { IconButton } from './IconButton';
import styles from './Composer.module.css';

export type ComposerProps = {
  value: string;
  onChange: (value: string, pastes: Record<string, string>) => void;
  onSend?: (text: string) => void;
  onCancel?: () => void;
  disabled?: boolean;
  busy?: boolean;
  mode?: SubmitMode;
  onModeChange?: (mode: SubmitMode) => void;
  /// 当前 session 已发送的用户消息，供 `↑` 回溯。
  history?: string[];
  /// 折叠的长粘贴块。发送时按占位符还原。
  pastes?: Record<string, string>;
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
  history = [],
  pastes = {},
}: ComposerProps): ReactElement {
  const [historyCursor, setHistoryCursor] = useState(-1);
  const draftRef = useRef(value);

  const update = (next: string, nextPastes: Record<string, string> = pastes): void => {
    setHistoryCursor(-1);
    draftRef.current = next;
    onChange(next, nextPastes);
  };

  const submit = (): void => {
    setHistoryCursor(-1);
    onSend?.(expandPastes(value, pastes));
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key === 'Enter' && !event.shiftKey) {
      event.preventDefault();
      submit();
      return;
    }
    if (
      event.key === 'ArrowUp' &&
      history.length > 0 &&
      (historyCursor >= 0 || event.currentTarget.selectionStart === 0)
    ) {
      event.preventDefault();
      if (historyCursor === -1) {
        draftRef.current = value;
      }
      const step = stepHistory(history, historyCursor, -1, draftRef.current);
      setHistoryCursor(step.cursor);
      onChange(step.text, pastes);
      return;
    }
    if (event.key === 'ArrowDown' && historyCursor >= 0) {
      event.preventDefault();
      const step = stepHistory(history, historyCursor, 1, draftRef.current);
      setHistoryCursor(step.cursor);
      onChange(step.text, pastes);
    }
  };

  const handlePaste = (event: ClipboardEvent<HTMLTextAreaElement>) => {
    const pasted = event.clipboardData.getData('text');
    const ordinal = Object.keys(pastes).length + 1;
    const folded = foldPaste(pasted, ordinal);
    if (folded === null) {
      return;
    }
    event.preventDefault();
    const start = event.currentTarget.selectionStart;
    const end = event.currentTarget.selectionEnd;
    update(`${value.slice(0, start)}${folded.token}${value.slice(end)}`, {
      ...pastes,
      [folded.token]: folded.text,
    });
  };

  return (
    <form
      className={styles.composer}
      onSubmit={(event) => {
        event.preventDefault();
        submit();
      }}
    >
      <textarea
        className={styles.input}
        aria-label="消息"
        value={value}
        placeholder={busy ? PLACEHOLDERS[mode] : '描述要完成的任务'}
        rows={3}
        disabled={disabled}
        onChange={(event) => { update(event.target.value); }}
        onKeyDown={handleKeyDown}
        onPaste={handlePaste}
      />
      <div className={styles.actions}>
        <span className={styles.hint}>Enter 发送 · Shift+Enter 换行 · ↑ 回溯 · Ctrl/⌘+K 命令</span>
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
