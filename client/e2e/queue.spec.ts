import { expect, test } from './support/fixtures';
import { assistantMessages, composer, sendMessage } from './support/ui';

test('运行中的 run 把新消息留在队列，移除后不再执行', async ({ page, app }) => {
  app.provider.push({ kind: 'hold', text: '长回答的第一段。' });

  const sessionId = await app.createSession(app.workspace);
  await page.goto(app.urlFor(sessionId));

  await sendMessage(page, '第一条消息');
  await expect(page.getByRole('button', { name: '取消' })).toBeVisible();
  await expect(page.getByText('执行中')).toBeVisible();

  await sendMessage(page, '第二条消息');
  await expect(page.getByText('暂无排队消息')).toHaveCount(0);
  await expect(page.getByRole('button', { name: '移除队列项' })).toHaveCount(1);

  await page.getByRole('button', { name: '移除队列项' }).click();

  await expect(page.getByText('暂无排队消息')).toBeVisible();
  await expect(page.getByRole('button', { name: '移除队列项' })).toHaveCount(0);

  app.provider.release();

  await expect(assistantMessages(page)).toHaveAttribute('data-state', 'complete');
  await expect(page.getByText('长回答的第一段。')).toBeVisible();
  await expect(page.getByText('第二条消息')).toHaveCount(0);
  expect(app.provider.requests()).toHaveLength(1);
});

test('取消运行中的 run 丢弃未定型的输出并继续执行队列', async ({ page, app }) => {
  app.provider.push({ kind: 'hold', text: '被打断的回答。' });
  app.provider.push({ kind: 'text', text: '队列里那条的回答。' });

  const sessionId = await app.createSession(app.workspace);
  await page.goto(app.urlFor(sessionId));

  await sendMessage(page, '需要中断的任务');
  await expect(page.getByRole('button', { name: '取消' })).toBeVisible();
  await expect(page.getByText('被打断的回答。')).toBeVisible();

  await sendMessage(page, '排队等待的任务');
  await expect(page.getByRole('button', { name: '移除队列项' })).toHaveCount(1);

  await page.getByRole('button', { name: '取消' }).click();

  await expect(page.getByText('被打断的回答。')).toHaveCount(0);
  await expect(page.getByText('已取消')).toHaveCount(0);

  await expect(page.getByText('队列里那条的回答。')).toBeVisible();
  await expect(assistantMessages(page)).toHaveCount(1);
  await expect(assistantMessages(page).last()).toHaveAttribute('data-state', 'complete');
  await expect(page.getByText('暂无排队消息')).toBeVisible();
  await expect(composer(page)).toBeVisible();
});
