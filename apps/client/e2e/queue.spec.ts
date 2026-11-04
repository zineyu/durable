import { expect, test } from './support/fixtures';
import { assistantMessages, composer, sendMessage } from './support/ui';

type ChatRequest = { messages: { role: string; content: string }[] };

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

test('run 进行中发送插话，在下一次模型请求生效', async ({ page, app }) => {
  app.provider.push({ kind: 'hold', text: '正在回答。' });
  app.provider.push({ kind: 'text', text: '按修正继续。' });

  const sessionId = await app.createSession(app.workspace);
  await page.goto(app.urlFor(sessionId));

  await sendMessage(page, '第一条消息');
  await expect(page.getByRole('button', { name: '取消' })).toBeVisible();
  await expect(page.getByText('正在回答。')).toBeVisible();

  await page.getByRole('button', { name: '插话' }).click();
  await sendMessage(page, '改成只改领域层');
  await expect(page.getByText('改成只改领域层').first()).toBeVisible();

  const strip = page
    .locator('section')
    .filter({ has: page.getByRole('heading', { name: '队列' }) });
  await expect(strip.getByText('插话', { exact: true })).toBeVisible();

  app.provider.release();

  await expect(page.getByText('按修正继续。')).toBeVisible();
  await expect(assistantMessages(page).last()).toHaveAttribute('data-state', 'complete');
  await expect(page.getByText('暂无排队消息')).toBeVisible();

  const requests = app.provider.requests() as ChatRequest[];
  expect(requests).toHaveLength(2);
  expect(requests[1]?.messages.at(-1)).toMatchObject({ role: 'user', content: '改成只改领域层' });
});

test('取消运行中的 run 保留部分输出为已中断并继续执行队列', async ({ page, app }) => {
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

  await expect(page.getByText('已取消')).toHaveCount(0);
  await expect(page.getByText('已中断')).toBeVisible();

  await expect(page.getByText('队列里那条的回答。')).toBeVisible();
  await expect(assistantMessages(page)).toHaveCount(2);
  await expect(assistantMessages(page).first()).toHaveAttribute('data-state', 'interrupted');
  await expect(assistantMessages(page).last()).toHaveAttribute('data-state', 'complete');
  await expect(page.getByText('暂无排队消息')).toBeVisible();
  await expect(composer(page)).toBeVisible();
});
