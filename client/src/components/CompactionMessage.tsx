import type { ReactElement } from 'react';
import type { ContentPart } from '../api/generated/ContentPart';
import type { PartSkeleton } from '../api/generated/PartSkeleton';
import { Markdown } from './Markdown';
import { MessagePending } from './MessagePending';
import styles from './CompactionMessage.module.css';
import { bodyText } from '../view/content';

export type CompactionMessageProps = {
  parts: ContentPart[] | null;
  skeleton: PartSkeleton[];
};

/// 链上的压缩消息：内容是此前对话的摘要，不是模型对用户说的话。
export function CompactionMessage({ parts, skeleton }: CompactionMessageProps): ReactElement {
  return (
    <section className={styles.message} aria-label="上下文摘要">
      <header className={styles.header}>
        <span className={styles.title}>上下文摘要</span>
        <span className={styles.hint}>此前对话已压缩，下方是摘要</span>
      </header>
      <div className={styles.body}>
        {parts === null ? <MessagePending parts={skeleton} /> : <Markdown text={bodyText(parts)} />}
      </div>
    </section>
  );
}
