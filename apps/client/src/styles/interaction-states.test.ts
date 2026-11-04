import { readFileSync, readdirSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const componentDirectory = decodeURIComponent(new URL('../components/', import.meta.url).pathname);

describe('交互状态', () => {
  it('具备 hover 状态的交互组件同时定义 active 状态', () => {
    const offenders = readdirSync(componentDirectory)
      .filter((name) => name.endsWith('.module.css'))
      .filter((name) => readFileSync(`${componentDirectory}${name}`, 'utf8').includes(':hover'))
      .filter((name) => !readFileSync(`${componentDirectory}${name}`, 'utf8').includes(':active'));
    expect(offenders).toEqual([]);
  });
});
