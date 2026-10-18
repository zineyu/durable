import styles from './CodeBlock.module.css';

export type CodeBlockProps = {
  code: string;
  language?: string;
};

export function CodeBlock({ code, language }: CodeBlockProps) {
  return (
    <figure className={styles.block}>
      {language !== undefined ? <figcaption className={styles.language}>{language}</figcaption> : null}
      <pre className={styles.code}>{code}</pre>
    </figure>
  );
}
