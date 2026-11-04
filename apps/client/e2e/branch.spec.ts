import { expect, test } from './support/fixtures';
import { assistantMessages, sendMessage } from './support/ui';

test('编辑历史用户消息产生兄弟分支并可来回切换', async ({ page, app }) => {
  app.provider.push({ kind: 'text', text: '第一版回答。' });
  app.provider.push({ kind: 'text', text: '第二版回答。' });

  const sessionId = await app.createSession(app.workspace);
  await page.goto(app.urlFor(sessionId));
  await sendMessage(page, '原始问题');
  await expect(page.getByText('第一版回答。')).toBeVisible();
  await expect(assistantMessages(page)).toHaveAttribute('data-state', 'complete');

  await page.getByRole('button', { name: '编辑消息' }).click();
  const dialog = page.getByRole('dialog', { name: '编辑用户消息' });
  await dialog.getByLabel('消息正文').fill('改写问题');
  await dialog.getByRole('button', { name: '确认编辑' }).click();
  await expect(dialog).toHaveCount(0);

  await expect(page.getByText('第二版回答。')).toBeVisible();
  await expect(page.getByText('第一版回答。')).toHaveCount(0);

  const userBranch = page.locator('article').filter({ hasText: '改写问题' });
  await expect(userBranch.getByText('2/2')).toBeVisible();

  await userBranch.getByRole('button', { name: '上一个分支' }).click();

  await expect(page.getByText('第一版回答。')).toBeVisible();
  await expect(page.locator('article').filter({ hasText: '原始问题' })).toBeVisible();
  await expect(page.getByText('第二版回答。')).toHaveCount(0);
});

test('重试 assistant 消息产生同父兄弟且旧回答保留在另一分支', async ({ page, app }) => {
  app.provider.push({ kind: 'text', text: '首次回答。' });
  app.provider.push({ kind: 'text', text: '重试后的回答。' });

  const sessionId = await app.createSession(app.workspace);
  await page.goto(app.urlFor(sessionId));
  await sendMessage(page, '需要重试的问题');
  await expect(page.getByText('首次回答。')).toBeVisible();
  await expect(assistantMessages(page)).toHaveAttribute('data-state', 'complete');

  await assistantMessages(page).getByRole('button', { name: '重试' }).click();
  const dialog = page.getByRole('dialog', { name: '确认重试' });
  await dialog.getByRole('button', { name: '重试' }).click();
  await expect(dialog).toHaveCount(0);

  await expect(page.getByText('重试后的回答。')).toBeVisible();
  await expect(page.getByText('首次回答。')).toHaveCount(0);
  await expect(assistantMessages(page).getByText('2/2')).toBeVisible();

  await assistantMessages(page).getByRole('button', { name: '上一个分支' }).click();
  await expect(page.getByText('首次回答。')).toBeVisible();
});
