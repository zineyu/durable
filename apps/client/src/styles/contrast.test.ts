import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const tokensPath = decodeURIComponent(new URL('./tokens.css', import.meta.url).pathname);
const source = readFileSync(tokensPath, 'utf8');

const darkMarker = "[data-theme='dark']";

function parseDeclarations(text: string): Map<string, string> {
  const declarations = new Map<string, string>();
  for (const match of text.matchAll(/--([a-zA-Z0-9-]+)\s*:\s*([^;]+);/g)) {
    const name = match[1];
    const value = match[2];
    if (name === undefined || value === undefined) {
      continue;
    }
    declarations.set(`--${name}`, value.trim());
  }
  return declarations;
}

const darkIndex = source.indexOf(darkMarker);
const light = parseDeclarations(source.slice(0, darkIndex));
const dark = new Map(light);
for (const [name, value] of parseDeclarations(source.slice(darkIndex))) {
  dark.set(name, value);
}

function resolve(declarations: Map<string, string>, value: string): string {
  const reference = /^var\(\s*(--[a-zA-Z0-9-]+)\s*\)$/.exec(value);
  if (reference === null) {
    return value;
  }
  const name = reference[1];
  if (name === undefined) {
    throw new Error(`无法解析 token 引用: ${value}`);
  }
  const next = declarations.get(name);
  if (next === undefined) {
    throw new Error(`未定义的 token: ${name}`);
  }
  return resolve(declarations, next);
}

function toRgb(value: string): [number, number, number] {
  const hex = /^#([0-9a-fA-F]{3}|[0-9a-fA-F]{6})$/.exec(value);
  if (hex !== null) {
    const raw = hex[1];
    if (raw === undefined) {
      throw new Error(`无法解析颜色值: ${value}`);
    }
    const digits = raw.length === 3 ? raw.replace(/./g, (digit) => digit + digit) : raw;
    return [
      Number.parseInt(digits.slice(0, 2), 16),
      Number.parseInt(digits.slice(2, 4), 16),
      Number.parseInt(digits.slice(4, 6), 16),
    ];
  }
  const rgb = /^rgb\(\s*(\d+)\s+(\d+)\s+(\d+)\s*\)$/.exec(value);
  if (rgb !== null) {
    const red = rgb[1];
    const green = rgb[2];
    const blue = rgb[3];
    if (red === undefined || green === undefined || blue === undefined) {
      throw new Error(`无法解析颜色值: ${value}`);
    }
    return [Number(red), Number(green), Number(blue)];
  }
  throw new Error(`无法解析颜色值: ${value}`);
}

function channelLuminance(channel: number): number {
  const normalized = channel / 255;
  return normalized <= 0.03928 ? normalized / 12.92 : ((normalized + 0.055) / 1.055) ** 2.4;
}

function luminance(color: [number, number, number]): number {
  return (
    0.2126 * channelLuminance(color[0]) +
    0.7152 * channelLuminance(color[1]) +
    0.0722 * channelLuminance(color[2])
  );
}

function contrast(declarations: Map<string, string>, foreground: string, background: string): number {
  const foregroundValue = declarations.get(foreground);
  const backgroundValue = declarations.get(background);
  if (foregroundValue === undefined || backgroundValue === undefined) {
    throw new Error(`缺少 token: ${foreground} 或 ${background}`);
  }
  const first = luminance(toRgb(resolve(declarations, foregroundValue)));
  const second = luminance(toRgb(resolve(declarations, backgroundValue)));
  const lighter = Math.max(first, second);
  const darker = Math.min(first, second);
  return (lighter + 0.05) / (darker + 0.05);
}

const backgrounds = [
  '--color-bg',
  '--color-surface-1',
  '--color-surface-2',
  '--color-surface-3',
  '--color-bubble',
];

describe('正文对比度', () => {
  for (const [theme, declarations] of [
    ['浅色', light],
    ['深色', dark],
  ] as const) {
    it(`${theme}主题正文文本对比度不低于 4.5:1`, () => {
      for (const background of backgrounds) {
        const ratio = contrast(declarations, '--color-text', background);
        expect(ratio, `${theme} --color-text on ${background}`).toBeGreaterThanOrEqual(4.5);
      }
    });
  }
});
