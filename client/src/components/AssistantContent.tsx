import type { ReactElement } from 'react';
import { Markdown } from './Markdown';
import { ToolCard } from './ToolCard';
import { toolCardModel } from '../view/tools';
import type { ToolCallRecord } from '../api/generated/ToolCallRecord';

export type AssistantContentProps = {
  text: string;
  toolCalls: ToolCallRecord[];
};

export function AssistantContent({ text, toolCalls }: AssistantContentProps): ReactElement {
  return (
    <>
      {text.length > 0 ? <Markdown text={text} /> : null}
      {toolCalls.map((record) => {
        const model = toolCardModel(record);
        return (
          <ToolCard
            key={model.id}
            name={model.name}
            state={model.state}
            summary={model.summary}
            input={model.input}
            {...(model.result === null ? {} : { result: model.result })}
          />
        );
      })}
    </>
  );
}
