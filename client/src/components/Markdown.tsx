import { Children, isValidElement } from 'react';
import type { ComponentProps, ReactElement, ReactNode } from 'react';
import ReactMarkdown from 'react-markdown';
import { CodeBlock } from './CodeBlock';
import styles from './Markdown.module.css';

export type MarkdownProps = {
  text: string;
};

type CodeElementProps = {
  className?: string;
  children?: ReactNode;
};

export function Markdown({ text }: MarkdownProps): ReactElement {
  return (
    <div className={styles.markdown}>
      <ReactMarkdown components={{ pre: Pre }}>{text}</ReactMarkdown>
    </div>
  );
}

function Pre({ children }: ComponentProps<'pre'>) {
  const child = Children.toArray(children)[0];
  const code = isValidElement<CodeElementProps>(child) ? child.props : null;
  const language = languageOf(code?.className);
  return (
    <CodeBlock
      code={codeText(code?.children ?? children)}
      {...(language === null ? {} : { language })}
    />
  );
}

function languageOf(className: string | undefined): string | null {
  const match = /language-(\S+)/.exec(className ?? '');
  return match?.[1] ?? null;
}

function codeText(children: ReactNode): string {
  return Children.toArray(children)
    .filter((child): child is string => typeof child === 'string')
    .join('')
    .replace(/\n$/, '');
}
