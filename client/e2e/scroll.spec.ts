import { expect, test } from './support/fixtures';
import { assistantMessages, composer, sendMessage } from './support/ui';

const longAnswer = Array.from(
  { length: 40 },
  (_, index) => `第 ${String(index + 1)} 行：滚动位置的恢复依赖稳定的内容高度。`,
).join('\n\n');

/// 规格 user story R-45：刷新后恢复选中 session 与滚动位置。
test('刷新后恢复同一 session 的 transcript 滚动位置', async ({ page, app }) => {
  app.provider.push({ kind: 'text', text: longAnswer });

  const sessionId = await app.createSession(app.workspace);
  await page.goto(app.urlFor(sessionId));
  await expect(composer(page)).toBeVisible();
  await sendMessage(page, '给出一段足够长的回答');
  await expect(assistantMessages(page).last()).toHaveAttribute('data-state', 'complete');
  await expect(page.getByText('第 40 行：滚动位置的恢复依赖稳定的内容高度。')).toBeVisible();

  const log = page.getByRole('log', { name: '对话记录' });
  await log.hover();
  await page.mouse.wheel(0, 240);
  await expect.poll(() => log.evaluate((element) => element.scrollTop)).toBeGreaterThan(0);
  const before = await log.evaluate((element) => element.scrollTop);

  await page.reload();

  await expect(composer(page)).toBeVisible();
  await expect(page.getByText('第 40 行：滚动位置的恢复依赖稳定的内容高度。')).toBeVisible();
  await expect.poll(() => log.evaluate((element) => element.scrollTop)).toBe(before);
});
