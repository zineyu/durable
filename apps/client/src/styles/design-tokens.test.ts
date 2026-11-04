import { readFileSync, readdirSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const componentDirectory = decodeURIComponent(new URL('../components/', import.meta.url).pathname);
const styleDirectory = decodeURIComponent(new URL('./', import.meta.url).pathname);

function readStyleDirectory(directory: string, suffix: string): Record<string, string> {
  return Object.fromEntries(
    readdirSync(directory)
      .filter((name) => name.endsWith(suffix))
      .map((name) => [name, readFileSync(`${directory}${name}`, 'utf8')]),
  );
}

const componentStyles = readStyleDirectory(componentDirectory, '.module.css');
const globalStyles = readStyleDirectory(styleDirectory, '.css');

const colorLiteral = /#[0-9a-fA-F]{3,8}\b|\brgba?\(|\bhsla?\(/;

// 断点条件无法引用 CSS 变量，不参与长度 token 检查。
function withoutAtRules(css: string): string {
  return css
    .split('\n')
    .filter((line) => !line.trimStart().startsWith('@'))
    .join('\n');
}

describe('设计 token', () => {
  it('提供颜色、字体与间距三类 token', () => {
    const tokens = globalStyles['tokens.css'];
    expect(tokens).toBeDefined();
    for (const family of ['--color-', '--font-', '--space-']) {
      expect(tokens, `缺少 ${family} token`).toContain(family);
    }
  });

  it('组件样式不含硬编码颜色值', () => {
    expect(Object.keys(componentStyles).length).toBeGreaterThan(0);
    for (const [path, css] of Object.entries(componentStyles)) {
      expect(colorLiteral.test(css), `${path} 含硬编码颜色字面量`).toBe(false);
    }
  });

  it('组件样式引用间距与半径 token，不含硬编码长度值', () => {
    for (const [path, css] of Object.entries(componentStyles)) {
      expect(withoutAtRules(css), `${path} 含硬编码长度值`).not.toMatch(/\d+(px|rem)\b/);
    }
  });

  it('全局定义 focus-visible 视觉状态', () => {
    expect(globalStyles['global.css']).toContain(':focus-visible');
  });
});
