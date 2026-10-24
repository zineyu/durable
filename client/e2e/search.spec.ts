import { expect, test } from './support/fixtures';
import { assistantMessages, sendMessage } from './support/ui';

test('session 内搜索跳到命中并可导出 Markdown', async ({ page, app }) => {
  app.provider.push({ kind: 'text', text: '迁移已经完成。' });
  const sessionId = await app.createSession(app.workspace);
  await page.goto(app.urlFor(sessionId));
  await sendMessage(page, '实现迁移');
  await expect(assistantMessages(page)).toHaveAttribute('data-state', 'complete');

  await page.getByRole('button', { name: '搜索消息' }).click();
  const search = page.getByRole('textbox', { name: '搜索消息' });
  await search.fill('迁移');
  await expect(page.getByText('1/2')).toBeVisible();
  await expect(page.locator('[data-search-hit="true"]')).toHaveCount(1);

  await search.press('Enter');
  await expect(page.getByText('2/2')).toBeVisible();
  await expect(page.locator('[data-search-hit="true"]')).toHaveCount(1);

  await page.getByRole('button', { name: '关闭搜索' }).click();
  const download = page.waitForEvent('download');
  await page.getByRole('button', { name: '导出 Markdown' }).click();
  expect((await download).suggestedFilename()).toMatch(/\.md$/);
});
