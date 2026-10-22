import type { ReactElement, ReactNode } from 'react';
import type { ContentPart } from '../api/generated/ContentPart';
import type { PartSkeleton } from '../api/generated/PartSkeleton';
import type { ToolResultState } from '../api/generated/ToolResultState';
import { Markdown } from './Markdown';
import { MessagePending } from './MessagePending';
import { ToolCard } from './ToolCard';
import styles from './AssistantContent.module.css';
import { toolCardModel } from '../view/tools';

export type AssistantContentProps = {
  /// 块正文；`null` 表示骨架已到、正文还在拉取。
  parts: ContentPart[] | null;
  skeleton: PartSkeleton[];
  toolResults: Record<string, ToolResultState>;
};

/// 按块序渲染一条 assistant 消息：推理折叠、正文、工具卡片。
export function AssistantContent({
  parts,
  skeleton,
  toolResults,
}: AssistantContentProps): ReactElement {
  return (
    <div className={styles.content}>
      {parts === null
        ? <MessagePending parts={skeleton} />
        : parts.map((part, index) => block(part, index, toolResults))}
    </div>
  );
}

function block(
  part: ContentPart,
  index: number,
  results: Record<string, ToolResultState>,
): ReactNode {
  const key = `${part.kind}-${String(index)}`;
  switch (part.kind) {
    case 'text':
      return <Markdown key={key} text={part.text} />;
    case 'reasoning':
      return (
        <details key={key} className={styles.reasoning}>
          <summary className={styles.summary}>推理过程</summary>
          <div className={styles.reasoningBody}>
            <Markdown text={part.text} />
          </div>
        </details>
      );
    case 'tool_call': {
      const model = toolCardModel(part, results[part.callId]);
      return (
        <ToolCard
          key={key}
          name={model.name}
          state={model.state}
          summary={model.summary}
          input={model.input}
          {...(model.result === null ? {} : { result: model.result })}
        />
      );
    }
    case 'tool_result':
    case 'image':
      return null;
    default: {
      const unhandled: never = part;
      throw new Error(`未处理的内容块: ${JSON.stringify(unhandled)}`);
    }
  }
}
