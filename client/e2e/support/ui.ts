import type { Locator, Page } from '@playwright/test';
import { expect } from '@playwright/test';

import type { Harness } from './harness';

/// 输入区。发送按钮与取消按钮也在同一区域。
export function composer(page: Page): Locator {
  return page.getByRole('textbox', { name: '消息' });
}

/// transcript 里的 assistant 消息。`data-state` 是 complete、streaming、cancelled 或 failed。
export function assistantMessages(page: Page): Locator {
  return page.locator('article[data-state]');
}

/// 侧栏。
export function sidebar(page: Page): Locator {
  return page.getByRole('complementary');
}

export function nodeSwitcher(page: Page): Locator {
  return page.getByRole('navigation', { name: '节点切换器' });
}

export function workspaceGroup(page: Page, path: string): Locator {
  return page.locator('section').filter({ has: page.getByText(path, { exact: true }) });
}

/// 打开给定 session 并等待节点已选中、输入区可见。
export async function openSession(page: Page, app: Harness, sessionId: string): Promise<void> {
  await page.goto(app.urlFor(sessionId));
  await expect(
    nodeSwitcher(page).getByRole('button', { name: app.nodeName }),
  ).toHaveAttribute('aria-pressed', 'true');
  await expect(composer(page)).toBeVisible();
}

/// 在输入区输入文本并按 Enter 提交。
export async function sendMessage(page: Page, text: string): Promise<void> {
  const input = composer(page);
  await input.fill(text);
  await input.press('Enter');
}
